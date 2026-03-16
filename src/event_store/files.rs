pub struct EventFileManager<S> {
    storage: S,
}

impl<S: crate::ports::StorageBackend> EventFileManager<S> {
    pub fn new(storage: S) -> Self {
        Self { storage }
    }

    pub fn get_event_file_path(
        &self,
        events_path: &str,
        position: i64,
    ) -> Result<alloc::string::String, crate::ports::Error> {
        if events_path.is_empty() {
            return Err(crate::ports::Error::IoError);
        }
        if position <= 0 {
            return Err(crate::ports::Error::IoError);
        }
        Ok(alloc::format!("{}/{:010}.json", events_path, position))
    }

    pub async fn event_file_exists(&self, events_path: &str, position: i64) -> bool {
        if let Ok(path) = self.get_event_file_path(events_path, position) {
            self.storage.read_file(&path).await.is_ok()
        } else {
            false
        }
    }

    pub async fn write_event_async(
        &self,
        events_path: &str,
        event: &crate::domain::EventRecord,
        allow_overwrite: bool,
    ) -> Result<(), crate::ports::Error> {
        if events_path.is_empty() {
            return Err(crate::ports::Error::IoError);
        }
        if event.position == 0 {
            return Err(crate::ports::Error::IoError);
        }

        let path = self.get_event_file_path(events_path, event.position as i64)?;

        if !allow_overwrite && self.storage.read_file(&path).await.is_ok() {
            return Ok(()); // idempotent write skip
        }

        let _ = self.storage.create_dir_all(events_path).await;

        let mut data = [0u8; 1024];
        let bytes_written = serde_json_core::to_slice(event, &mut data)
            .map_err(|_| crate::ports::Error::IoError)?;

        self.storage
            .write_file(&path, &data[..bytes_written])
            .await?;
        Ok(())
    }

    pub async fn read_event_async(
        &self,
        events_path: &str,
        position: i64,
    ) -> Result<crate::domain::EventRecord, crate::ports::Error> {
        let path = self.get_event_file_path(events_path, position)?;
        let data = self.storage.read_file(&path).await?;

        match serde_json_core::from_slice::<crate::domain::EventRecord>(&data) {
            Ok((evt, _)) => Ok(evt),
            Err(_) => Err(crate::ports::Error::IoError),
        }
    }

    pub async fn read_events_async(
        &self,
        events_path: &str,
        positions: &[i64],
    ) -> Result<alloc::vec::Vec<crate::domain::EventRecord>, crate::ports::Error> {
        if events_path.is_empty() {
            return Err(crate::ports::Error::IoError);
        }
        let mut results = alloc::vec::Vec::new();

        for &pos in positions {
            let evt = self.read_event_async(events_path, pos).await?;
            results.push(evt);
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{DomainEvent, EventRecord, Tag};
    use crate::event_store::*;
    use crate::ports::tests::InMemoryStorage;

    fn create_test_event(
        position: u64,
        event_type: &str,
        data: &str,
    ) -> crate::domain::EventRecord {
        crate::domain::EventRecord {
            position,
            event_id: alloc::string::String::from("id"),
            event: crate::domain::DomainEvent {
                event_type: alloc::string::String::from(event_type),
                data: alloc::string::String::from(data),
                tags: alloc::vec::Vec::new(),
            },
            metadata: None,
            timestamp: 0,
        }
    }

    fn create_test_event_record(position: u64, event_type: &str, tags: Vec<Tag>) -> EventRecord {
        EventRecord {
            position,
            event_id: alloc::format!("evt-{}", position),
            event: DomainEvent {
                event_type: event_type.to_string(),
                data: "{}".to_string(),
                tags,
            },
            metadata: None,
            timestamp: 123456789,
        }
    }

    #[tokio::test]
    async fn event_file_manager_write_event_creates_file() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = EventFileManager::new(storage.clone());
        let evt = create_test_event(1, "Test", "data");

        manager
            .write_event_async("/events", &evt, false)
            .await
            .unwrap();

        assert!(storage.read_file("/events/0000000001.json").await.is_ok());
    }

    #[tokio::test]
    async fn event_file_manager_write_event_zero_position_fails() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = EventFileManager::new(storage.clone());
        let evt = create_test_event(0, "Test", "data");

        let res = manager.write_event_async("/events", &evt, false).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn event_file_manager_write_skips_if_exists() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = EventFileManager::new(storage.clone());
        let evt1 = create_test_event(1, "Test1", "data");
        let evt2 = create_test_event(1, "Test2", "data");

        manager
            .write_event_async("/events", &evt1, false)
            .await
            .unwrap();
        manager
            .write_event_async("/events", &evt2, false)
            .await
            .unwrap(); // should skip

        let read = manager.read_event_async("/events", 1).await.unwrap();
        assert_eq!(read.event.event_type, "Test1");
    }

    #[tokio::test]
    async fn event_file_manager_write_overwrites_if_allowed() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = EventFileManager::new(storage.clone());
        let evt1 = create_test_event(1, "Test1", "data");
        let evt2 = create_test_event(1, "Test2", "data");

        manager
            .write_event_async("/events", &evt1, false)
            .await
            .unwrap();
        manager
            .write_event_async("/events", &evt2, true)
            .await
            .unwrap(); // allowed

        let read = manager.read_event_async("/events", 1).await.unwrap();
        assert_eq!(read.event.event_type, "Test2");
    }

    #[tokio::test]
    async fn event_file_manager_read_event_returns_correct() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = EventFileManager::new(storage.clone());
        let evt = create_test_event(42, "Test", "data");

        manager
            .write_event_async("/events", &evt, false)
            .await
            .unwrap();

        let read = manager.read_event_async("/events", 42).await.unwrap();
        assert_eq!(read.position, 42);
        assert_eq!(read.event.event_type, "Test");
    }

    #[tokio::test]
    async fn event_file_manager_read_missing_fails() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = EventFileManager::new(storage.clone());

        let res = manager.read_event_async("/events", 999).await;
        assert!(matches!(res, Err(crate::ports::Error::NotFound)));
    }

    #[tokio::test]
    async fn event_file_manager_read_events_returns_multiple_in_order() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = EventFileManager::new(storage.clone());

        manager
            .write_event_async("/events", &create_test_event(1, "T1", "d"), false)
            .await
            .unwrap();
        manager
            .write_event_async("/events", &create_test_event(2, "T2", "d"), false)
            .await
            .unwrap();
        manager
            .write_event_async("/events", &create_test_event(3, "T3", "d"), false)
            .await
            .unwrap();

        let events = manager
            .read_events_async("/events", &[3, 1, 2])
            .await
            .unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].position, 3);
        assert_eq!(events[1].position, 1);
        assert_eq!(events[2].position, 2);
    }

    #[tokio::test]
    async fn event_file_manager_get_event_file_path() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = EventFileManager::new(storage.clone());

        let path = manager.get_event_file_path("/test", 5).unwrap();
        assert_eq!(path, "/test/0000000005.json");

        assert!(manager.get_event_file_path("", 5).is_err());
        assert!(manager.get_event_file_path("/test", 0).is_err());
        assert!(manager.get_event_file_path("/test", -1).is_err());
    }

    #[tokio::test]
    async fn event_file_manager_event_file_exists() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = EventFileManager::new(storage.clone());

        manager
            .write_event_async("/events", &create_test_event(1, "T1", "d"), false)
            .await
            .unwrap();

        assert!(manager.event_file_exists("/events", 1).await);
        assert!(!manager.event_file_exists("/events", 2).await);
        assert!(!manager.event_file_exists("/events", 0).await);
        assert!(!manager.event_file_exists("/events", -1).await);
    }
}
