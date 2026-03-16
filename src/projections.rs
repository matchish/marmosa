use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use crate::domain::{EventRecord, Query};
use crate::ports::{Error, StorageBackend};

/// Defines how events are projected into a materialized view state
pub trait ProjectionDefinition {
    /// The projection state type
    type State;

    /// Unique name identifying this projection
    fn projection_name(&self) -> &str;

    /// Query filter for events this projection cares about
    fn event_types(&self) -> Query;

    /// Extracts the key from an event to identify which projection instance to update
    fn key_selector(&self, event: &EventRecord) -> Option<String>;

    /// Applies an event to the current state, returning the new state
    fn apply(&self, state: Option<Self::State>, event: &EventRecord) -> Option<Self::State>;
}

/// Storage interface for projection state - can be implemented for different backends
pub trait ProjectionStore<TState> {
    fn get(&self, key: &str) -> impl core::future::Future<Output = Result<Option<TState>, Error>> + Send;

    fn get_all(&self) -> impl core::future::Future<Output = Result<Vec<TState>, Error>> + Send;

    fn save(&self, key: &str, state: &TState) -> impl core::future::Future<Output = Result<(), Error>> + Send;

    fn delete(&self, key: &str) -> impl core::future::Future<Output = Result<(), Error>> + Send;
}

/// Checkpoint for tracking projection progress
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectionCheckpoint {
    pub projection_name: String,
    pub last_position: u64,
}

/// Runs a projection by reading events and applying them to the store
pub struct ProjectionRunner<S, TState, P, Store> 
where 
    P: ProjectionDefinition<State = TState>,
    Store: ProjectionStore<TState>,
{
    storage: S,
    projection: P,
    store: Store,
    _marker: core::marker::PhantomData<TState>,
}

impl<S, TState, P, Store> ProjectionRunner<S, TState, P, Store>
where
    S: StorageBackend + Send + Sync,
    TState: Serialize + for<'de> Deserialize<'de> + Send + Sync,
    P: ProjectionDefinition<State = TState> + Send + Sync,
    Store: ProjectionStore<TState> + Send + Sync,
{
    pub fn new(storage: S, projection: P, store: Store) -> Self {
        Self {
            storage,
            projection,
            store,
            _marker: core::marker::PhantomData,
        }
    }

    fn checkpoint_path(&self) -> String {
        alloc::format!("Projections/_checkpoints/{}.json", self.projection.projection_name())
    }

    pub async fn get_checkpoint(&self) -> Result<Option<u64>, Error> {
        let path = self.checkpoint_path();
        match self.storage.read_file(&path).await {
            Ok(data) => {
                let (checkpoint, _) = serde_json_core::from_slice::<ProjectionCheckpoint>(&data)
                    .map_err(|_| Error::IoError)?;
                Ok(Some(checkpoint.last_position))
            }
            Err(Error::NotFound) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub async fn save_checkpoint(&self, position: u64) -> Result<(), Error> {
        let checkpoint = ProjectionCheckpoint {
            projection_name: alloc::string::String::from(self.projection.projection_name()),
            last_position: position,
        };
        
        let _ = self.storage.create_dir_all("Projections/_checkpoints").await;
        let path = self.checkpoint_path();
        let data = serde_json_core::to_vec::<_, 512>(&checkpoint)
            .map_err(|_| Error::IoError)?;
        self.storage.write_file(&path, &data).await
    }

    /// Process a batch of events starting from the last checkpoint
    pub async fn process_events(&self, events: &[EventRecord]) -> Result<u64, Error> {
        let query = self.projection.event_types();
        let mut last_position = self.get_checkpoint().await?.unwrap_or(0);

        for event in events {
            // Skip events we've already processed
            if event.position <= last_position && last_position > 0 {
                continue;
            }

            // Check if this event matches our query
            if !query.matches(event) {
                last_position = event.position;
                continue;
            }

            // Get the key for this event
            if let Some(key) = self.projection.key_selector(event) {
                // Load current state
                let current_state = self.store.get(&key).await?;
                
                // Apply event
                if let Some(new_state) = self.projection.apply(current_state, event) {
                    self.store.save(&key, &new_state).await?;
                }
            }

            last_position = event.position;
        }

        // Save checkpoint
        if last_position > 0 {
            self.save_checkpoint(last_position).await?;
        }

        Ok(last_position)
    }
}

/// Default projection store backed by StorageBackend (file system, KV, etc.)
pub struct StorageBackendProjectionStore<S, TState> {
    storage: S,
    projection_name: String,
    _marker: core::marker::PhantomData<TState>,
}

impl<S, TState> StorageBackendProjectionStore<S, TState> {
    pub fn new(storage: S, projection_name: String) -> Self {
        Self {
            storage,
            projection_name,
            _marker: core::marker::PhantomData,
        }
    }

    fn get_projection_path(&self) -> String {
        alloc::format!("Projections/{}", self.projection_name)
    }

    fn get_file_path(&self, key: &str) -> String {
        alloc::format!("Projections/{}/{}.json", self.projection_name, key)
    }
}

impl<S: StorageBackend + Send + Sync, TState: Serialize + for<'de> Deserialize<'de> + Send + Sync> ProjectionStore<TState>
    for StorageBackendProjectionStore<S, TState>
{
    async fn get(&self, key: &str) -> Result<Option<TState>, Error> {
        let path = self.get_file_path(key);
        match self.storage.read_file(&path).await {
            Ok(data) => {
                let (state, _) = serde_json_core::from_slice::<TState>(&data)
                    .map_err(|_| Error::IoError)?;
                Ok(Some(state))
            }
            Err(Error::NotFound) => Ok(None),
            Err(e) => Err(e),
        }
    }

    async fn get_all(&self) -> Result<Vec<TState>, Error> {
        let dir_path = self.get_projection_path();
        let files = self.storage.read_dir(&dir_path).await.unwrap_or_default();
        
        let mut results = Vec::new();
        for file_path in files {
            if file_path.ends_with(".json") {
                if let Ok(data) = self.storage.read_file(&file_path).await {
                    if let Ok((state, _)) = serde_json_core::from_slice::<TState>(&data) {
                        results.push(state);
                    }
                }
            }
        }
        Ok(results)
    }

    async fn save(&self, key: &str, state: &TState) -> Result<(), Error> {
        let dir_path = self.get_projection_path();
        let _ = self.storage.create_dir_all(&dir_path).await;
        
        let path = self.get_file_path(key);
        let data = serde_json_core::to_vec::<_, 4096>(state)
            .map_err(|_| Error::IoError)?;
        self.storage.write_file(&path, &data).await
    }

    async fn delete(&self, key: &str) -> Result<(), Error> {
        let path = self.get_file_path(key);
        self.storage.delete_file(&path).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use alloc::vec;
    use crate::domain::{DomainEvent, Tag};
    use crate::ports::tests::InMemoryStorage;

    // A simple counter projection for testing
    #[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
    struct CounterState {
        key: String,
        count: u64,
    }

    struct CounterProjection;

    impl ProjectionDefinition for CounterProjection {
        type State = CounterState;

        fn projection_name(&self) -> &str {
            "CounterProjection"
        }

        fn event_types(&self) -> Query {
            Query {
                items: vec![crate::domain::QueryItem {
                    event_types: vec!["CounterIncremented".to_string()],
                    tags: vec![],
                }],
            }
        }

        fn key_selector(&self, event: &EventRecord) -> Option<String> {
            // Extract key from tags
            event.event.tags.iter()
                .find(|t| t.key == "counter_id")
                .map(|t| t.value.clone())
        }

        fn apply(&self, state: Option<Self::State>, event: &EventRecord) -> Option<Self::State> {
            let key = self.key_selector(event)?;
            let mut current = state.unwrap_or_else(|| CounterState { key: key.clone(), count: 0 });
            
            if event.event.event_type == "CounterIncremented" {
                current.count += 1;
            }
            
            Some(current)
        }
    }

    #[test]
    fn test_projection_definition_returns_name() {
        let projection = CounterProjection;
        assert_eq!(projection.projection_name(), "CounterProjection");
    }

    #[test]
    fn test_projection_definition_returns_event_types_query() {
        let projection = CounterProjection;
        let query = projection.event_types();
        assert_eq!(query.items.len(), 1);
        assert_eq!(query.items[0].event_types, vec!["CounterIncremented".to_string()]);
    }

    #[test]
    fn test_projection_apply_creates_new_state() {
        let projection = CounterProjection;
        
        let event = EventRecord {
            position: 0,
            event_id: "evt-1".to_string(),
            event: DomainEvent {
                event_type: "CounterIncremented".to_string(),
                data: "{}".to_string(),
                tags: vec![Tag { key: "counter_id".to_string(), value: "counter-1".to_string() }],
            },
            metadata: None,
            timestamp: 1000,
        };

        let new_state = projection.apply(None, &event);
        
        assert!(new_state.is_some());
        let state = new_state.unwrap();
        assert_eq!(state.key, "counter-1");
        assert_eq!(state.count, 1);
    }

    #[test]
    fn test_projection_apply_updates_existing_state() {
        let projection = CounterProjection;
        
        let existing = CounterState { key: "counter-1".to_string(), count: 5 };
        
        let event = EventRecord {
            position: 1,
            event_id: "evt-2".to_string(),
            event: DomainEvent {
                event_type: "CounterIncremented".to_string(),
                data: "{}".to_string(),
                tags: vec![Tag { key: "counter_id".to_string(), value: "counter-1".to_string() }],
            },
            metadata: None,
            timestamp: 2000,
        };

        let new_state = projection.apply(Some(existing), &event);
        
        assert!(new_state.is_some());
        let state = new_state.unwrap();
        assert_eq!(state.count, 6);
    }

    #[test]
    fn test_key_selector_extracts_from_tags() {
        let projection = CounterProjection;
        
        let event = EventRecord {
            position: 0,
            event_id: "evt-1".to_string(),
            event: DomainEvent {
                event_type: "CounterIncremented".to_string(),
                data: "{}".to_string(),
                tags: vec![Tag { key: "counter_id".to_string(), value: "my-counter".to_string() }],
            },
            metadata: None,
            timestamp: 1000,
        };

        let key = projection.key_selector(&event);
        assert_eq!(key, Some("my-counter".to_string()));
    }

    #[test]
    fn test_key_selector_returns_none_when_tag_missing() {
        let projection = CounterProjection;
        
        let event = EventRecord {
            position: 0,
            event_id: "evt-1".to_string(),
            event: DomainEvent {
                event_type: "CounterIncremented".to_string(),
                data: "{}".to_string(),
                tags: vec![],  // No tags
            },
            metadata: None,
            timestamp: 1000,
        };

        let key = projection.key_selector(&event);
        assert_eq!(key, None);
    }

    // StorageBackendProjectionStore tests

    #[tokio::test]
    async fn test_projection_store_save_and_get() {
        let storage = InMemoryStorage::new();
        let store: StorageBackendProjectionStore<_, CounterState> = 
            StorageBackendProjectionStore::new(storage, "CounterProjection".to_string());

        let state = CounterState { key: "counter-1".to_string(), count: 42 };
        store.save("counter-1", &state).await.unwrap();

        let retrieved = store.get("counter-1").await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().count, 42);
    }

    #[tokio::test]
    async fn test_projection_store_get_returns_none_for_missing() {
        let storage = InMemoryStorage::new();
        let store: StorageBackendProjectionStore<_, CounterState> = 
            StorageBackendProjectionStore::new(storage, "CounterProjection".to_string());

        let retrieved = store.get("nonexistent").await.unwrap();
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_projection_store_delete() {
        let storage = InMemoryStorage::new();
        let store: StorageBackendProjectionStore<_, CounterState> = 
            StorageBackendProjectionStore::new(storage, "CounterProjection".to_string());

        let state = CounterState { key: "counter-1".to_string(), count: 10 };
        store.save("counter-1", &state).await.unwrap();

        // Verify it exists
        assert!(store.get("counter-1").await.unwrap().is_some());

        // Delete it
        store.delete("counter-1").await.unwrap();

        // Verify it's gone
        assert!(store.get("counter-1").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_projection_store_get_all() {
        let storage = InMemoryStorage::new();
        let store: StorageBackendProjectionStore<_, CounterState> = 
            StorageBackendProjectionStore::new(storage, "CounterProjection".to_string());

        store.save("counter-1", &CounterState { key: "counter-1".to_string(), count: 1 }).await.unwrap();
        store.save("counter-2", &CounterState { key: "counter-2".to_string(), count: 2 }).await.unwrap();
        store.save("counter-3", &CounterState { key: "counter-3".to_string(), count: 3 }).await.unwrap();

        let all = store.get_all().await.unwrap();
        assert_eq!(all.len(), 3);
        
        // Verify total count
        let total: u64 = all.iter().map(|s| s.count).sum();
        assert_eq!(total, 6);
    }

    // ProjectionRunner tests

    #[tokio::test]
    async fn test_projection_runner_processes_events() {
        let storage = InMemoryStorage::new();
        let store: StorageBackendProjectionStore<InMemoryStorage, CounterState> = 
            StorageBackendProjectionStore::new(InMemoryStorage::new(), "CounterProjection".to_string());
        
        let runner = ProjectionRunner::new(storage, CounterProjection, store);

        let events = vec![
            EventRecord {
                position: 1,
                event_id: "evt-1".to_string(),
                event: DomainEvent {
                    event_type: "CounterIncremented".to_string(),
                    data: "{}".to_string(),
                    tags: vec![Tag { key: "counter_id".to_string(), value: "counter-1".to_string() }],
                },
                metadata: None,
                timestamp: 1000,
            },
            EventRecord {
                position: 2,
                event_id: "evt-2".to_string(),
                event: DomainEvent {
                    event_type: "CounterIncremented".to_string(),
                    data: "{}".to_string(),
                    tags: vec![Tag { key: "counter_id".to_string(), value: "counter-1".to_string() }],
                },
                metadata: None,
                timestamp: 2000,
            },
        ];

        let last_pos = runner.process_events(&events).await.unwrap();
        assert_eq!(last_pos, 2);

        // Verify the state was updated
        let state = runner.store.get("counter-1").await.unwrap();
        assert!(state.is_some());
        assert_eq!(state.unwrap().count, 2);
    }

    #[tokio::test]
    async fn test_projection_runner_saves_checkpoint() {
        let storage = InMemoryStorage::new();
        let store: StorageBackendProjectionStore<InMemoryStorage, CounterState> = 
            StorageBackendProjectionStore::new(InMemoryStorage::new(), "CounterProjection".to_string());
        
        let runner = ProjectionRunner::new(storage, CounterProjection, store);

        let events = vec![
            EventRecord {
                position: 5,
                event_id: "evt-5".to_string(),
                event: DomainEvent {
                    event_type: "CounterIncremented".to_string(),
                    data: "{}".to_string(),
                    tags: vec![Tag { key: "counter_id".to_string(), value: "counter-1".to_string() }],
                },
                metadata: None,
                timestamp: 5000,
            },
        ];

        runner.process_events(&events).await.unwrap();

        // Verify checkpoint was saved
        let checkpoint = runner.get_checkpoint().await.unwrap();
        assert_eq!(checkpoint, Some(5));
    }

    #[tokio::test]
    async fn test_projection_runner_skips_already_processed_events() {
        let storage = InMemoryStorage::new();
        let store: StorageBackendProjectionStore<InMemoryStorage, CounterState> = 
            StorageBackendProjectionStore::new(InMemoryStorage::new(), "CounterProjection".to_string());
        
        let runner = ProjectionRunner::new(storage, CounterProjection, store);

        // First batch
        let events1 = vec![
            EventRecord {
                position: 1,
                event_id: "evt-1".to_string(),
                event: DomainEvent {
                    event_type: "CounterIncremented".to_string(),
                    data: "{}".to_string(),
                    tags: vec![Tag { key: "counter_id".to_string(), value: "counter-1".to_string() }],
                },
                metadata: None,
                timestamp: 1000,
            },
        ];

        runner.process_events(&events1).await.unwrap();
        
        // Second batch includes the same event plus a new one
        let events2 = vec![
            EventRecord {
                position: 1,
                event_id: "evt-1".to_string(),
                event: DomainEvent {
                    event_type: "CounterIncremented".to_string(),
                    data: "{}".to_string(),
                    tags: vec![Tag { key: "counter_id".to_string(), value: "counter-1".to_string() }],
                },
                metadata: None,
                timestamp: 1000,
            },
            EventRecord {
                position: 2,
                event_id: "evt-2".to_string(),
                event: DomainEvent {
                    event_type: "CounterIncremented".to_string(),
                    data: "{}".to_string(),
                    tags: vec![Tag { key: "counter_id".to_string(), value: "counter-1".to_string() }],
                },
                metadata: None,
                timestamp: 2000,
            },
        ];

        runner.process_events(&events2).await.unwrap();

        // Should have count of 2, not 3 (event at position 1 was skipped)
        let state = runner.store.get("counter-1").await.unwrap();
        assert_eq!(state.unwrap().count, 2);
    }

    #[tokio::test]
    async fn test_projection_runner_filters_by_event_type() {
        let storage = InMemoryStorage::new();
        let store: StorageBackendProjectionStore<InMemoryStorage, CounterState> = 
            StorageBackendProjectionStore::new(InMemoryStorage::new(), "CounterProjection".to_string());
        
        let runner = ProjectionRunner::new(storage, CounterProjection, store);

        let events = vec![
            EventRecord {
                position: 1,
                event_id: "evt-1".to_string(),
                event: DomainEvent {
                    event_type: "CounterIncremented".to_string(),
                    data: "{}".to_string(),
                    tags: vec![Tag { key: "counter_id".to_string(), value: "counter-1".to_string() }],
                },
                metadata: None,
                timestamp: 1000,
            },
            EventRecord {
                position: 2,
                event_id: "evt-2".to_string(),
                event: DomainEvent {
                    event_type: "SomeOtherEvent".to_string(),  // Different event type
                    data: "{}".to_string(),
                    tags: vec![Tag { key: "counter_id".to_string(), value: "counter-1".to_string() }],
                },
                metadata: None,
                timestamp: 2000,
            },
            EventRecord {
                position: 3,
                event_id: "evt-3".to_string(),
                event: DomainEvent {
                    event_type: "CounterIncremented".to_string(),
                    data: "{}".to_string(),
                    tags: vec![Tag { key: "counter_id".to_string(), value: "counter-1".to_string() }],
                },
                metadata: None,
                timestamp: 3000,
            },
        ];

        runner.process_events(&events).await.unwrap();

        // Should have count of 2 (only CounterIncremented events)
        let state = runner.store.get("counter-1").await.unwrap();
        assert_eq!(state.unwrap().count, 2);
    }
}
