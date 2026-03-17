pub mod files;
pub mod indices;
pub mod initializer;
pub mod ledger;
pub mod options;

pub use files::*;
pub use indices::*;
pub use initializer::*;
pub use ledger::*;
pub use options::*;

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
        options: Option<Vec<crate::domain::ReadOption>>,
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
        options: Option<Vec<crate::domain::ReadOption>>,
    ) -> Result<Vec<EventRecord>, Error> {
        let dir_path = "Events";
        let mut results = Vec::new();

        let sequence_files = self.storage.read_dir(dir_path).await.unwrap_or_default();
        let mut available_positions: Vec<u64> = sequence_files
            .iter()
            .filter_map(|f| f.split('/').last()?.strip_suffix(".json")?.parse::<u64>().ok())
            .collect();
        available_positions.sort_unstable();

        let start = start_position.map(|p| p + 1).unwrap_or(1); // Exclusive bound

        let descending_opt = crate::domain::ReadOption::DESCENDING;
        let is_descending = options.as_ref().map_or(false, |opts| opts.contains(&descending_opt));
        
        let mut filtered_positions: Vec<u64> = available_positions.into_iter().filter(|&p| p >= start).collect();
        if is_descending {
            filtered_positions.reverse();
        }

        for current_pos in filtered_positions {
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
            let mut sequence = existing_files
                .iter()
                .filter_map(|f| f.split('/').last()?.strip_suffix(".json")?.parse::<u64>().ok())
                .max()
                .map(|v| v + 1)
                .unwrap_or(1);

            if let Some(cond) = condition {
                // Read all current events to check against condition
                let current_events = self
                    .read_internal(Query::all(), cond.after_sequence_position, None, None)
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
        options: Option<Vec<crate::domain::ReadOption>>,
    ) -> Result<Vec<EventRecord>, Error> {
        self.read_internal(query, start_position, max_count, options).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::DomainEvent;
    use crate::ports::tests::{FakeClock, InMemoryStorage};

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
            .read_async(Query::all(), None, Some(10), None)
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
                event: DomainEvent {
                    event_type: "TestEvent".to_string(),
                    data: "1".to_string(),
                    tags: vec![],
                },
                metadata: None,
            },
            EventData {
                event_id: "evt-2".to_string(),
                event: DomainEvent {
                    event_type: "TestEvent".to_string(),
                    data: "2".to_string(),
                    tags: vec![],
                },
                metadata: None,
            },
            EventData {
                event_id: "evt-3".to_string(),
                event: DomainEvent {
                    event_type: "TestEvent".to_string(),
                    data: "3".to_string(),
                    tags: vec![],
                },
                metadata: None,
            },
        ];

        store.append_async(events, None).await.unwrap();

        let result = store.read_async(Query::all(), None, None, None).await.unwrap();
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
                event: DomainEvent {
                    event_type: "Type1".to_string(),
                    data: "1".to_string(),
                    tags: vec![],
                },
                metadata: None,
            },
            EventData {
                event_id: "evt-2".to_string(),
                event: DomainEvent {
                    event_type: "Type2".to_string(),
                    data: "2".to_string(),
                    tags: vec![],
                },
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
            event: DomainEvent {
                event_type: "TestEvent".to_string(),
                data: "1".to_string(),
                tags: vec![],
            },
            metadata: None,
        }];
        store.append_async(batch1, None).await.unwrap();

        let batch2 = vec![EventData {
            event_id: "evt-2".to_string(),
            event: DomainEvent {
                event_type: "TestEvent".to_string(),
                data: "2".to_string(),
                tags: vec![],
            },
            metadata: None,
        }];
        store.append_async(batch2, None).await.unwrap();

        let result = store.read_async(Query::all(), None, None, None).await.unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].position, 0);
        assert_eq!(result[1].position, 1);
    }

    #[tokio::test]
    async fn test_read_async_with_query_all_returns_all_events() {
        let storage = InMemoryStorage::new();
        let clock = FakeClock::new(1696000000);
        let store = OpossumStore::new(storage, clock);

        for i in 1..=3 {
            let evt = EventData {
                event_id: alloc::format!("evt-{}", i),
                event: DomainEvent {
                    event_type: "TestEvent".to_string(),
                    data: alloc::format!("Data{}", i),
                    tags: alloc::vec::Vec::new(),
                },
                metadata: None,
            };
            store
                .append_async(alloc::vec::Vec::from([evt]), None)
                .await
                .unwrap();
        }

        let events = store.read_async(Query::all(), None, None, None).await.unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].position, 0);
        assert_eq!(events[1].position, 1);
        assert_eq!(events[2].position, 2);
    }

    #[tokio::test]
    async fn test_read_async_with_single_event_type_returns_matching_events() {
        let storage = InMemoryStorage::new();
        let clock = FakeClock::new(1696000000);
        let store = OpossumStore::new(storage, clock);

        for event_type in ["OrderCreated", "OrderShipped", "OrderCreated"] {
            let evt = EventData {
                event_id: "id".to_string(),
                event: DomainEvent {
                    event_type: event_type.to_string(),
                    data: "test".to_string(),
                    tags: alloc::vec::Vec::new(),
                },
                metadata: None,
            };
            store
                .append_async(alloc::vec::Vec::from([evt]), None)
                .await
                .unwrap();
        }

        let query = Query {
            items: alloc::vec::Vec::from([crate::domain::QueryItem {
                event_types: alloc::vec::Vec::from(["OrderCreated".to_string()]),
                tags: alloc::vec::Vec::new(),
            }]),
        };

        let events = store.read_async(query, None, None, None).await.unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event.event_type, "OrderCreated");
        assert_eq!(events[1].event.event_type, "OrderCreated");
        assert_eq!(events[0].position, 0);
        assert_eq!(events[1].position, 2);
    }

    #[tokio::test]
    async fn test_read_async_with_single_tag_returns_matching_events() {
        let storage = InMemoryStorage::new();
        let clock = FakeClock::new(1696000000);
        let store = OpossumStore::new(storage, clock);

        let tag_prod = crate::domain::Tag {
            key: "Environment".to_string(),
            value: "Production".to_string(),
        };
        let tag_dev = crate::domain::Tag {
            key: "Environment".to_string(),
            value: "Development".to_string(),
        };

        for tag in [tag_prod.clone(), tag_dev, tag_prod.clone()] {
            let evt = EventData {
                event_id: "id".to_string(),
                event: DomainEvent {
                    event_type: "TestEvent".to_string(),
                    data: "test".to_string(),
                    tags: alloc::vec::Vec::from([tag]),
                },
                metadata: None,
            };
            store
                .append_async(alloc::vec::Vec::from([evt]), None)
                .await
                .unwrap();
        }

        let query = Query {
            items: alloc::vec::Vec::from([crate::domain::QueryItem {
                event_types: alloc::vec::Vec::new(),
                tags: alloc::vec::Vec::from([tag_prod]),
            }]),
        };

        let events = store.read_async(query, None, None, None).await.unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].position, 0);
        assert_eq!(events[1].position, 2);
    }

    #[tokio::test]
    async fn test_read_async_with_from_position_in_middle_returns_only_events_after_position() {
        let storage = InMemoryStorage::new();
        let clock = FakeClock::new(1696000000);
        let store = OpossumStore::new(storage, clock);

        for i in 1..=5 {
            let evt = EventData {
                event_id: alloc::format!("evt-{}", i),
                event: DomainEvent {
                    event_type: "TestEvent".to_string(),
                    data: alloc::format!("Data{}", i),
                    tags: alloc::vec::Vec::new(),
                },
                metadata: None,
            };
            store
                .append_async(alloc::vec::Vec::from([evt]), None)
                .await
                .unwrap();
        }

        let events = store.read_async(Query::all(), Some(1), None, None).await.unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].position, 2);
        assert_eq!(events[1].position, 3);
        assert_eq!(events[2].position, 4);
    }
}
