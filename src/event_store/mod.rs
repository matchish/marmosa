pub mod files;
pub mod indices;
pub mod initializer;
pub mod ledger;

pub use files::*;
pub use indices::*;
pub use initializer::*;
pub use ledger::*;

use alloc::format;
use alloc::vec::Vec;
use serde_json_core::to_vec;

use crate::domain::AppendCondition;
use crate::domain::EventData;
use crate::domain::EventRecord;
use crate::domain::Query;
use crate::ports::{Clock, Error, StorageBackend};

pub trait EventStore {
    fn append_async(
        &self,
        events: Vec<EventData>,
        condition: Option<AppendCondition>,
    ) -> impl core::future::Future<Output = Result<(), Error>> + Send;

    fn read_async(
        &self,
        query: Query,
        start_position: Option<u64>,
        max_count: Option<usize>,
    ) -> impl core::future::Future<Output = Result<Vec<EventRecord>, Error>> + Send;
}

pub type MarmosaStore<S, C> = OpossumStore<S, C>;

pub struct OpossumStore<S, C> {
    storage: S,
    clock: C,
}

impl<S, C> OpossumStore<S, C> {
    pub fn new(storage: S, clock: C) -> Self {
        Self { storage, clock }
    }
}

impl<S: StorageBackend + Send + Sync, C: Clock + Send + Sync> OpossumStore<S, C> {
    async fn read_internal(
        &self,
        query: Query,
        start_position: Option<u64>,
        max_count: Option<usize>,
    ) -> Result<Vec<EventRecord>, Error> {
        let dir_path = "Events";
        let mut results = Vec::new();

        let sequence_files = self.storage.read_dir(dir_path).await.unwrap_or_default();
        let total_count = sequence_files.len() as u64;

        let start = start_position.map(|p| p + 1).unwrap_or(0); // Exclusive bound

        for current_pos in start..total_count {
            let file_path = format!("{}/{:010}.json", dir_path, current_pos);
            let data = self.storage.read_file(&file_path).await?;
            let (record, _) =
                serde_json_core::from_slice::<EventRecord>(&data).map_err(|_| Error::IoError)?;

            if query.matches(&record) {
                results.push(record);
                if let Some(max) = max_count {
                    if results.len() >= max {
                        break;
                    }
                }
            }
        }

        Ok(results)
    }
}

impl<S: StorageBackend + Send + Sync, C: Clock + Send + Sync> EventStore for OpossumStore<S, C> {
    async fn append_async(
        &self,
        events: Vec<EventData>,
        condition: Option<AppendCondition>,
    ) -> Result<(), Error> {
        let global_lock = "global_store";
        self.storage.acquire_stream_lock(global_lock).await?;

        let result = async {
            let dir_path = "Events";
            let _ = self.storage.create_dir_all(&dir_path).await; // Ignore if exists

            let existing_files = self.storage.read_dir(&dir_path).await.unwrap_or_default();
            let mut sequence = existing_files.len() as u64;

            if let Some(cond) = condition {
                // Read all current events to check against condition
                let current_events = self
                    .read_internal(Query::all(), cond.after_sequence_position, None)
                    .await?;
                for evt in current_events {
                    if cond.fail_if_events_match.matches(&evt) {
                        return Err(Error::AppendConditionFailed);
                    }
                }
            }

            let timestamp = self.clock.now_millis();

            for event in events {
                let record = EventRecord {
                    position: sequence,
                    event_id: event.event_id,
                    event: event.event,
                    metadata: event.metadata,
                    timestamp,
                };

                let vec = to_vec::<_, 4096>(&record).map_err(|_| Error::IoError)?; // Map error appropriately

                let file_path = format!("{}/{:010}.json", dir_path, sequence);
                self.storage.write_file(&file_path, &vec).await?;

                sequence += 1;
            }

            Ok(())
        }
        .await;

        let _ = self.storage.release_stream_lock(global_lock).await;
        result
    }

    async fn read_async(
        &self,
        query: Query,
        start_position: Option<u64>,
        max_count: Option<usize>,
    ) -> Result<Vec<EventRecord>, Error> {
        self.read_internal(query, start_position, max_count).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{DomainEvent, EventRecord, Tag};
    use crate::ports::tests::{FakeClock, InMemoryStorage};

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
    async fn test_append_single_event_to_new_stream() {
        let storage = InMemoryStorage::new();
        let clock = FakeClock::new(1696000000);
        let store = OpossumStore::new(storage, clock);

        let event = EventData {
            event_id: "evt-123".to_string(),
            event: DomainEvent {
                event_type: "TestEvent".to_string(),
                data: "{\"foo\":\"bar\"}".to_string(),
                tags: vec![],
            },
            metadata: None,
        };

        let result = store.append_async(vec![event], None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_append_and_read_stream() {
        let storage = InMemoryStorage::new();
        let clock = FakeClock::new(1696000000);
        let store = OpossumStore::new(storage, clock);

        let event1 = EventData {
            event_id: "evt-1".to_string(),
            event: DomainEvent {
                event_type: "TestEvent".to_string(),
                data: "{}".to_string(),
                tags: vec![],
            },
            metadata: None,
        };
        store.append_async(vec![event1], None).await.unwrap();

        let events = store
            .read_async(Query::all(), None, Some(10))
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_id, "evt-1");
        assert_eq!(events[0].position, 0);
    }

    #[tokio::test]
    async fn test_append_condition_stream_does_not_exist() {
        let storage = InMemoryStorage::new();
        let clock = FakeClock::new(1696000000);
        let store = OpossumStore::new(storage, clock);

        let event = EventData {
            event_id: "evt-1".to_string(),
            event: DomainEvent {
                event_type: "TestEvent".to_string(),
                data: "{}".to_string(),
                tags: vec![],
            },
            metadata: None,
        };

        let cond = AppendCondition {
            fail_if_events_match: Query::all(),
            after_sequence_position: None,
        };

        // First append should succeed
        let res = store.append_async(vec![event], Some(cond.clone())).await;
        assert!(res.is_ok());

        let event2 = EventData {
            event_id: "evt-2".to_string(),
            event: DomainEvent {
                event_type: "TestEvent".to_string(),
                data: "{}".to_string(),
                tags: vec![],
            },
            metadata: None,
        };

        // Second append should fail because events exist
        let res = store.append_async(vec![event2], Some(cond)).await;
        assert_eq!(res, Err(Error::AppendConditionFailed));
    }

    #[tokio::test]
    async fn test_append_condition_expected_position() {
        let storage = InMemoryStorage::new();
        let clock = FakeClock::new(1696000000);
        let store = OpossumStore::new(storage, clock);

        let event = EventData {
            event_id: "evt-1".to_string(),
            event: DomainEvent {
                event_type: "TestEvent".to_string(),
                data: "{}".to_string(),
                tags: vec![],
            },
            metadata: None,
        };

        // Append at position 0 (first event)
        let res = store.append_async(vec![event], None).await;
        assert!(res.is_ok());

        let event2 = EventData {
            event_id: "evt-2".to_string(),
            event: DomainEvent {
                event_type: "TestEvent".to_string(),
                data: "{}".to_string(),
                tags: vec![],
            },
            metadata: None,
        };

        let cond = AppendCondition {
            fail_if_events_match: Query::all(),
            after_sequence_position: Some(0),
        };
        let res = store.append_async(vec![event2], Some(cond)).await;
        assert!(res.is_ok());

        let event3 = EventData {
            event_id: "evt-3".to_string(),
            event: DomainEvent {
                event_type: "TestEvent".to_string(),
                data: "{}".to_string(),
                tags: vec![],
            },
            metadata: None,
        };

        let failing_cond = AppendCondition {
            fail_if_events_match: Query::all(),
            after_sequence_position: Some(0),
        };

        // Attempting to append expecting position 0 again should fail because there is an event at pos 1
        let res = store.append_async(vec![event3], Some(failing_cond)).await;
        assert_eq!(res, Err(Error::AppendConditionFailed));
    }

    #[tokio::test]
    async fn test_append_with_multiple_events_assigns_sequential_positions() {
        let storage = InMemoryStorage::new();
        let clock = FakeClock::new(1696000000);
        let store = OpossumStore::new(storage, clock);

        let events = vec![
            EventData {
                event_id: "evt-1".to_string(),
                event: DomainEvent { event_type: "TestEvent".to_string(), data: "1".to_string(), tags: vec![] },
                metadata: None,
            },
            EventData {
                event_id: "evt-2".to_string(),
                event: DomainEvent { event_type: "TestEvent".to_string(), data: "2".to_string(), tags: vec![] },
                metadata: None,
            },
            EventData {
                event_id: "evt-3".to_string(),
                event: DomainEvent { event_type: "TestEvent".to_string(), data: "3".to_string(), tags: vec![] },
                metadata: None,
            },
        ];

        store.append_async(events, None).await.unwrap();

        let result = store.read_async(Query::all(), None, None).await.unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].position, 0);
        assert_eq!(result[1].position, 1);
        assert_eq!(result[2].position, 2);
        assert_eq!(result[0].event.data, "1");
        assert_eq!(result[1].event.data, "2");
        assert_eq!(result[2].event.data, "3");
    }

    #[tokio::test]
    async fn test_append_writes_all_event_files() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let clock = FakeClock::new(1696000000);
        let store = OpossumStore::new(storage.clone(), clock);

        let events = vec![
            EventData {
                event_id: "evt-1".to_string(),
                event: DomainEvent { event_type: "Type1".to_string(), data: "1".to_string(), tags: vec![] },
                metadata: None,
            },
            EventData {
                event_id: "evt-2".to_string(),
                event: DomainEvent { event_type: "Type2".to_string(), data: "2".to_string(), tags: vec![] },
                metadata: None,
            },
        ];

        store.append_async(events, None).await.unwrap();

        let files = storage.read_dir("Events").await.unwrap();
        assert!(files.contains(&"Events/0000000000.json".to_string()));
        assert!(files.contains(&"Events/0000000000.json".to_string()));
    }

        #[tokio::test]
    async fn test_append_multiple_sequential_appends_maintains_continuous_sequence() {
        let storage = InMemoryStorage::new();
        let clock = FakeClock::new(1696000000);
        let store = OpossumStore::new(storage, clock);

        let batch1 = vec![EventData {
            event_id: "evt-1".to_string(),
            event: DomainEvent { event_type: "TestEvent".to_string(), data: "1".to_string(), tags: vec![] },
            metadata: None,
        }];
        store.append_async(batch1, None).await.unwrap();

        let batch2 = vec![EventData {
            event_id: "evt-2".to_string(),
            event: DomainEvent { event_type: "TestEvent".to_string(), data: "2".to_string(), tags: vec![] },
            metadata: None,
        }];
        store.append_async(batch2, None).await.unwrap();

        let result = store.read_async(Query::all(), None, None).await.unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].position, 0);
        assert_eq!(result[1].position, 1);
    }
}

pub mod read_tests;
