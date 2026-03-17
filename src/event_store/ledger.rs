#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct LedgerData {
    pub last_sequence_position: u64,
    pub event_count: u64,
}

pub struct LedgerManager<S> {
    storage: S,
}

impl<S: crate::ports::StorageBackend + Send + Sync + Clone> LedgerManager<S> {
    pub fn new(storage: S) -> Self {
        Self { storage }
    }

    fn ledger_path(&self, context_path: &str) -> alloc::string::String {
        alloc::format!("{}/.ledger", context_path)
    }

    pub async fn get_last_sequence_position_async(
        &self,
        context_path: &str,
    ) -> Result<u64, crate::ports::Error> {
        let path = self.ledger_path(context_path);
        match self.storage.read_file(&path).await {
            Ok(data) => {
                if data.is_empty() {
                    return Ok(0);
                }
                match serde_json_core::from_slice::<LedgerData>(&data) {
                    Ok((ledger, _)) => Ok(ledger.last_sequence_position),
                    Err(_) => Err(crate::ports::Error::IoError), // Corrupt ledger
                }
            }
            Err(crate::ports::Error::NotFound) => Ok(0),
            Err(e) => Err(e),
        }
    }

    pub async fn get_next_sequence_position_async(
        &self,
        context_path: &str,
    ) -> Result<u64, crate::ports::Error> {
        let last = self.get_last_sequence_position_async(context_path).await?;
        if last == 0
            && self
                .storage
                .read_file(&self.ledger_path(context_path))
                .await
                == Err(crate::ports::Error::NotFound)
        {
            Ok(1)
        } else {
            Ok(last + 1)
        }
    }

    pub async fn update_sequence_position_async(
        &self,
        context_path: &str,
        position: u64,
    ) -> Result<(), crate::ports::Error> {
        let _ = self.storage.create_dir_all(context_path).await;
        let path = self.ledger_path(context_path);
        let data = LedgerData {
            last_sequence_position: position,
            event_count: position,
        };
        let vec =
            serde_json_core::to_vec::<_, 1024>(&data).map_err(|_| crate::ports::Error::IoError)?;
        self.storage.write_file(&path, &vec).await?;
        Ok(())
    }

    pub async fn acquire_lock_async(&self, context_path: &str) -> Result<(), crate::ports::Error> {
        self.storage.acquire_stream_lock(context_path).await
    }

    pub async fn release_lock_async(&self, context_path: &str) -> Result<(), crate::ports::Error> {
        self.storage.release_stream_lock(context_path).await
    }

    pub async fn reconcile_ledger_async(
        &self,
        context_path: &str,
        events_dir: &str,
    ) -> Result<(), crate::ports::Error> {
        let files = match self.storage.read_dir(events_dir).await {
            Ok(f) => f,
            Err(_) => return Ok(()), // no dir means nothing to reconcile
        };

        let max_pos = files
            .iter()
            .filter_map(|f| {
                f.split('/')
                    .next_back()?
                    .strip_suffix(".json")?
                    .parse::<u64>()
                    .ok()
            })
            .max()
            .unwrap_or(0);

        let last = self.get_last_sequence_position_async(context_path).await?;
        if max_pos > last {
            self.update_sequence_position_async(context_path, max_pos)
                .await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_store::*;
    use crate::ports::tests::InMemoryStorage;

    #[tokio::test]
    async fn storage_initializer_get_paths() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let options = OpossumOptions::new("/test_root").use_store("CourseManagement");
        let initializer = StorageInitializer::new(storage, options);

        assert_eq!(
            initializer.get_context_path("CourseManagement"),
            "/test_root/CourseManagement"
        );
        assert_eq!(
            initializer.get_events_path("CourseManagement"),
            "/test_root/CourseManagement/Events"
        );
        assert_eq!(
            initializer.get_ledger_path("CourseManagement"),
            "/test_root/CourseManagement/.ledger"
        );
        assert_eq!(
            initializer.get_event_type_index_path("CourseManagement"),
            "/test_root/CourseManagement/Indices/EventType"
        );
        assert_eq!(
            initializer.get_tags_index_path("CourseManagement"),
            "/test_root/CourseManagement/Indices/Tags"
        );
    }

    #[tokio::test]
    async fn ledger_get_last_sequence_position_when_no_exists_returns_zero() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = LedgerManager::new(storage.clone());
        let pos = manager
            .get_last_sequence_position_async("/temp")
            .await
            .unwrap();
        assert_eq!(pos, 0);
    }

    #[tokio::test]
    async fn ledger_get_last_sequence_position_when_exists_returns_last() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = LedgerManager::new(storage.clone());
        manager
            .update_sequence_position_async("/temp", 42)
            .await
            .unwrap();
        let pos = manager
            .get_last_sequence_position_async("/temp")
            .await
            .unwrap();
        assert_eq!(pos, 42);
    }

    #[tokio::test]
    async fn ledger_get_last_sequence_position_when_empty_returns_zero() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = LedgerManager::new(storage.clone());
        let _ = storage.create_dir_all("/temp").await;
        let _ = storage.write_file("/temp/.ledger", b"").await;

        let pos = manager
            .get_last_sequence_position_async("/temp")
            .await
            .unwrap();
        assert_eq!(pos, 0);
    }

    #[tokio::test]
    async fn ledger_get_last_sequence_position_when_corrupted_errors() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = LedgerManager::new(storage.clone());
        let _ = storage.create_dir_all("/temp").await;
        let _ = storage
            .write_file("/temp/.ledger", b"{ invalid JSON }")
            .await;
        let p = manager.get_last_sequence_position_async("/temp").await;
        assert!(p.is_err());
    }

    #[tokio::test]
    async fn ledger_get_next_sequence_position_when_not_exists_returns_one() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = LedgerManager::new(storage.clone());
        let pos = manager
            .get_next_sequence_position_async("/temp")
            .await
            .unwrap();
        assert_eq!(pos, 1);
    }

    #[tokio::test]
    async fn ledger_get_next_sequence_position_when_exists_returns_incremented() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = LedgerManager::new(storage.clone());
        manager
            .update_sequence_position_async("/temp", 42)
            .await
            .unwrap();
        let pos = manager
            .get_next_sequence_position_async("/temp")
            .await
            .unwrap();
        assert_eq!(pos, 43); // Because it was at 42, returning last + 1
    }

    #[tokio::test]
    async fn ledger_update_sequence_position_overwrites() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = LedgerManager::new(storage.clone());
        manager
            .update_sequence_position_async("/temp", 10)
            .await
            .unwrap();
        manager
            .update_sequence_position_async("/temp", 20)
            .await
            .unwrap();
        let pos = manager
            .get_last_sequence_position_async("/temp")
            .await
            .unwrap();
        assert_eq!(pos, 20);
    }

    #[tokio::test]
    async fn ledger_update_sequence_position_with_zero_succeeds() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = LedgerManager::new(storage.clone());

        manager
            .update_sequence_position_async("/temp", 0)
            .await
            .unwrap();

        let pos = manager
            .get_last_sequence_position_async("/temp")
            .await
            .unwrap();
        assert_eq!(pos, 0);
    }

    #[tokio::test]
    async fn ledger_acquire_lock_prevents_simultaneous() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = alloc::sync::Arc::new(LedgerManager::new(storage.clone()));

        manager.acquire_lock_async("/temp").await.unwrap();

        // Spawn a task that tries to acquire lock and fails using tokio::time::timeout
        let manager2 = manager.clone();
        let handle = tokio::spawn(async move {
            let res = tokio::time::timeout(
                core::time::Duration::from_millis(50),
                manager2.acquire_lock_async("/temp"),
            )
            .await;
            res.is_err() // Should timeout meaning it was blocked
        });

        let did_timeout = handle.await.unwrap();
        assert!(did_timeout, "Should have timed out waiting for lock");

        manager.release_lock_async("/temp").await.unwrap();
        assert!(manager.acquire_lock_async("/temp").await.is_ok());
    }

    #[tokio::test]
    async fn ledger_sequential_operations_work_correctly() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = LedgerManager::new(storage.clone());

        // Initial state
        let pos1 = manager
            .get_next_sequence_position_async("/temp")
            .await
            .unwrap();
        assert_eq!(pos1, 1);

        // Update to position 1
        manager
            .update_sequence_position_async("/temp", 1)
            .await
            .unwrap();

        // Next should be 2
        let pos2 = manager
            .get_next_sequence_position_async("/temp")
            .await
            .unwrap();
        assert_eq!(pos2, 2);

        // Update to position 2
        manager
            .update_sequence_position_async("/temp", 2)
            .await
            .unwrap();

        // Next should be 3
        let pos3 = manager
            .get_next_sequence_position_async("/temp")
            .await
            .unwrap();
        assert_eq!(pos3, 3);

        // Last should be 2
        let last = manager
            .get_last_sequence_position_async("/temp")
            .await
            .unwrap();
        assert_eq!(last, 2);
    }

    #[tokio::test]
    async fn ledger_persists_across_manager_instances() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager1 = LedgerManager::new(storage.clone());
        let manager2 = LedgerManager::new(storage.clone());

        manager1
            .update_sequence_position_async("/temp", 100)
            .await
            .unwrap();
        let position = manager2
            .get_last_sequence_position_async("/temp")
            .await
            .unwrap();

        assert_eq!(position, 100);
    }

    #[tokio::test]
    async fn ledger_file_has_correct_format() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = LedgerManager::new(storage.clone());

        manager
            .update_sequence_position_async("/temp", 42)
            .await
            .unwrap();

        let content = storage.read_file("/temp/.ledger").await.unwrap();
        let content_str = core::str::from_utf8(&content).unwrap();

        // Ensure the fields are as expected in JSON
        assert!(
            content_str.contains("LastSequencePosition")
                || content_str.contains("last_sequence_position")
                || content_str.contains("LastSequencePosition")
                || content_str.contains("lastSequencePosition")
        );
        assert!(content_str.contains("42"));
    }

    #[tokio::test]
    async fn ledger_reconcile_when_events_exist_beyond_ledger() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = LedgerManager::new(storage.clone());

        manager
            .update_sequence_position_async("/temp", 3)
            .await
            .unwrap();
        let _ = storage.create_dir_all("/temp/events").await;
        // Write event files directly
        let _ = storage
            .write_file("/temp/events/0000000004.json", b"{}")
            .await;
        let _ = storage
            .write_file("/temp/events/0000000005.json", b"{}")
            .await;

        manager
            .reconcile_ledger_async("/temp", "/temp/events")
            .await
            .unwrap();

        let pos = manager
            .get_last_sequence_position_async("/temp")
            .await
            .unwrap();
        assert_eq!(pos, 5);
    }

    #[tokio::test]
    async fn ledger_reconcile_noop_when_correct() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = LedgerManager::new(storage.clone());

        manager
            .update_sequence_position_async("/temp", 5)
            .await
            .unwrap();
        let _ = storage.create_dir_all("/temp/events").await;
        let _ = storage
            .write_file("/temp/events/0000000003.json", b"{}")
            .await;
        let _ = storage
            .write_file("/temp/events/0000000005.json", b"{}")
            .await;

        manager
            .reconcile_ledger_async("/temp", "/temp/events")
            .await
            .unwrap();

        let pos = manager
            .get_last_sequence_position_async("/temp")
            .await
            .unwrap();
        assert_eq!(pos, 5);
    }

    #[tokio::test]
    async fn ledger_reconcile_handles_empty_events() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = LedgerManager::new(storage.clone());

        manager
            .update_sequence_position_async("/temp", 3)
            .await
            .unwrap();
        let _ = storage.create_dir_all("/temp/events").await;

        manager
            .reconcile_ledger_async("/temp", "/temp/events")
            .await
            .unwrap();

        let pos = manager
            .get_last_sequence_position_async("/temp")
            .await
            .unwrap();
        assert_eq!(pos, 3);
    }

    #[tokio::test]
    async fn ledger_reconcile_handles_non_existent_events_directory() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = LedgerManager::new(storage.clone());

        manager
            .update_sequence_position_async("/temp", 3)
            .await
            .unwrap();

        // events path "/temp/events" does not exist

        manager
            .reconcile_ledger_async("/temp", "/temp/events")
            .await
            .unwrap();

        let pos = manager
            .get_last_sequence_position_async("/temp")
            .await
            .unwrap();
        assert_eq!(pos, 3);
    }
}
