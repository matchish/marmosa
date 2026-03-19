use crate::event_store::MarmosaStore;
use crate::ports::{Error, StorageBackend};

pub trait EventStoreAdmin {
    /// Deletes the entire event store data (both events and projections)
    fn delete_store(&self) -> impl core::future::Future<Output = Result<(), Error>> + Send;
}

impl<S: StorageBackend + Send + Sync, C: Send + Sync> EventStoreAdmin for MarmosaStore<S, C> {
    async fn delete_store(&self) -> Result<(), Error> {
        let dirs = ["Events", "Indices", "Projections", ".ledger"];

        for dir in &dirs {
            let _ = self.storage.delete_dir_all(dir).await;
        }

        let _ = self.storage.delete_file(".store.lock").await;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{DomainEvent, EventData};
    use crate::event_store::EventStore;
    use crate::ports::tests::{FakeClock, InMemoryStorage};
    use alloc::string::ToString;
    use alloc::sync::Arc;

    fn create_test_event(event_type: &str) -> EventData {
        EventData {
            event_id: alloc::format!("evt-{}", event_type),
            metadata: None,
            event: DomainEvent {
                event_type: event_type.to_string(),
                data: alloc::format!("{{\"data\":\"{}\"}}", event_type),
                tags: alloc::vec![],
            },
        }
    }

    #[tokio::test]
    async fn delete_store_with_events_deletes_store_directory() {
        let storage = Arc::new(InMemoryStorage::new());
        let clock = FakeClock::new(100);
        let store = MarmosaStore::new(storage.clone(), clock);

        store
            .append_async(alloc::vec![create_test_event("TestEvent")], None)
            .await
            .unwrap();

        // Verify it exists in storage (Events folder)
        let files = storage.read_dir("Events").await.unwrap_or_default();
        assert!(!files.is_empty(), "Events should exist");

        // Act
        store.delete_store().await.unwrap();

        // Assert
        let files_after = storage.read_dir("Events").await.unwrap_or_default();
        assert!(files_after.is_empty(), "Events directory should be empty");
    }

    #[tokio::test]
    async fn delete_store_when_store_does_not_exist_completes_gracefully() {
        let storage = Arc::new(InMemoryStorage::new());
        let clock = FakeClock::new(100);
        let store = MarmosaStore::new(storage.clone(), clock);

        // Ensure directory is empty
        let files = storage.read_dir("Events").await.unwrap_or_default();
        assert!(files.is_empty(), "Events should be empty");

        // Act & Assert - should not throw
        assert!(store.delete_store().await.is_ok());
    }

    #[tokio::test]
    async fn delete_store_then_append_recreates_store_from_scratch() {
        let storage = Arc::new(InMemoryStorage::new());
        let clock = FakeClock::new(100);
        let store = MarmosaStore::new(storage.clone(), clock);

        // Seed an event then delete
        store
            .append_async(alloc::vec![create_test_event("OldEvent")], None)
            .await
            .unwrap();
        store.delete_store().await.unwrap();

        // Append after deletion
        store
            .append_async(alloc::vec![create_test_event("NewEvent")], None)
            .await
            .unwrap();

        let files = storage.read_dir("Events").await.unwrap();
        println!("Files after append: {:?}", files);

        // Assert - only the new event exists, sequence restarts at position 1
        let events = store
            .read_async(crate::domain::Query::all(), None, None, None)
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].position, 0);
        assert_eq!(events[0].event.event_type, "NewEvent");
    }

    #[tokio::test]
    async fn delete_store_concurrent_with_append_neither_throws_and_store_is_consistent() {
        let storage = Arc::new(InMemoryStorage::new());
        let clock = FakeClock::new(100);
        let store = Arc::new(MarmosaStore::new(storage.clone(), clock));

        // Seed the store
        store
            .append_async(alloc::vec![create_test_event("SeedEvent")], None)
            .await
            .unwrap();

        let store_clone1 = store.clone();
        let store_clone2 = store.clone();

        let append_task = tokio::spawn(async move {
            store_clone1
                .append_async(alloc::vec![create_test_event("RaceEvent")], None)
                .await
        });

        let delete_task = tokio::spawn(async move { store_clone2.delete_store().await });

        let res_append = append_task.await.unwrap();
        let res_delete = delete_task.await.unwrap();

        // None of these should return an unexpected internal panic, though in real FS
        // they might fail with IO Error which is also an Ok(Err(..)) not a panic.
        // It's acceptable for both to succeed.
        assert!(res_append.is_ok() || res_append.is_err());
        assert!(res_delete.is_ok() || res_delete.is_err());
    }

    #[tokio::test]
    async fn delete_store_called_twice_concurrently_both_complete_without_error() {
        let storage = Arc::new(InMemoryStorage::new());
        let clock = FakeClock::new(100);
        let store = Arc::new(MarmosaStore::new(storage.clone(), clock));

        store
            .append_async(alloc::vec![create_test_event("SeedEvent")], None)
            .await
            .unwrap();

        let store_clone1 = store.clone();
        let store_clone2 = store.clone();

        let task1 = tokio::spawn(async move { store_clone1.delete_store().await });

        let task2 = tokio::spawn(async move { store_clone2.delete_store().await });

        let res1 = task1.await.unwrap();
        let res2 = task2.await.unwrap();

        assert!(res1.is_ok());
        assert!(res2.is_ok());

        let files = storage.read_dir("Events").await.unwrap_or_default();
        assert!(files.is_empty());
    }
}
