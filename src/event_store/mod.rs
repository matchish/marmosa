//! Append-only event store API and default file-backed implementation.
//!
//! # Overview
//!
//! The event store persists immutable domain events in commit order. Current read models are
//! derived from these events (for example, with projections), rather than being the source of
//! truth themselves.
//!
//! The core API has two operations:
//! - [`EventStore::append_async`] for writing one batch atomically.
//! - [`EventStore::read_async`] for querying events by type/tags via [`crate::domain::Query`].
//!
//! # Storage Notes
//!
//! The default implementation writes events as JSON files under the `Events` directory and uses
//! zero-padded file names (for example `0000000000.json`, `0000000001.json`).
//!
//! Related modules in this namespace also manage:
//! - event/tag indices (`indices`),
//! - store bootstrapping (`initializer`),
//! - and ledger metadata (`ledger`).
//!
//! # Examples
//!
//! ```rust
//! use marmosa::domain::{Query, QueryItem, Tag};
//!
//! let query = Query {
//!     items: vec![QueryItem {
//!         event_types: vec!["StudentRegisteredEvent".to_string()],
//!         tags: vec![Tag {
//!             key: "studentId".to_string(),
//!             value: "abc123".to_string(),
//!         }],
//!     }],
//! };
//!
//! assert_eq!(query.items.len(), 1);
//! assert_eq!(query.items[0].event_types[0], "StudentRegisteredEvent");
//! ```

pub mod admin;
pub mod files;
pub mod indices;
pub mod initializer;
pub mod ledger;
pub mod options;

pub use admin::*;
pub use files::*;
pub use indices::*;
pub use initializer::*;
pub use ledger::*;
pub use options::*;

use alloc::format;
use alloc::vec::Vec;

use crate::domain::AppendCondition;
use crate::domain::EventData;
use crate::domain::EventRecord;
use crate::domain::Query;
use crate::ports::{Clock, Error, StorageBackend};

/// Append/read contract for an event store.
pub trait EventStore {
    /// Appends a batch of events.
    ///
    /// If `condition` is provided, the append succeeds only when no conflicting events were
    /// committed after the expected position.
    ///
    /// # Errors
    ///
    /// Returns [`Error::AppendConditionFailed`] when optimistic concurrency fails, or an I/O
    /// related error when persistence fails.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use marmosa::domain::{DomainEvent, EventData};
    /// use marmosa::event_store::EventStore;
    /// use marmosa::ports::Error;
    ///
    /// async fn append_registration<S: EventStore>(store: &S) -> Result<(), Error> {
    ///     let event = EventData {
    ///         event_id: "evt-1".to_string(),
    ///         event: DomainEvent {
    ///             event_type: "StudentRegisteredEvent".to_string(),
    ///             data: "{\"studentId\":\"abc123\"}".to_string(),
    ///             tags: vec![],
    ///         },
    ///         metadata: None,
    ///     };
    ///
    ///     store.append_async(vec![event], None).await
    /// }
    /// ```
    fn append_async(
        &self,
        events: Vec<EventData>,
        condition: Option<AppendCondition>,
    ) -> impl core::future::Future<Output = Result<(), Error>> + Send;

    /// Reads events matching `query`.
    ///
    /// `start_position` is exclusive, `max_count` limits returned events, and `options` can
    /// influence ordering.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use marmosa::domain::Query;
    /// use marmosa::event_store::EventStore;
    /// use marmosa::ports::Error;
    ///
    /// async fn read_recent<S: EventStore>(store: &S) -> Result<(), Error> {
    ///     let events = store.read_async(Query::all(), Some(10), Some(50), None).await?;
    ///     assert!(events.len() <= 50);
    ///     Ok(())
    /// }
    /// ```
    fn read_async(
        &self,
        query: Query,
        start_position: Option<u64>,
        max_count: Option<usize>,
        options: Option<Vec<crate::domain::ReadOption>>,
    ) -> impl core::future::Future<Output = Result<Vec<EventRecord>, Error>> + Send;
}

/// Default event store implementation backed by a [`StorageBackend`].
pub struct MarmosaStore<S, C> {
    storage: S,
    clock: C,
}

impl<S, C> MarmosaStore<S, C> {
    /// Creates a new event store using the provided storage backend and clock.
    pub fn new(storage: S, clock: C) -> Self {
        Self { storage, clock }
    }
}

impl<S: StorageBackend + Send + Sync + Clone, C: Clock + Send + Sync> MarmosaStore<S, C> {
    /// Returns all event positions by scanning the Events directory.
    async fn get_all_positions(&self, start_position: Option<u64>) -> Result<Vec<u64>, Error> {
        let dir_path = "Events";
        let sequence_files = self.storage.read_dir(dir_path).await.unwrap_or_default();
        let mut positions: Vec<u64> = sequence_files
            .iter()
            .filter_map(|f| {
                f.split('/')
                    .next_back()?
                    .strip_suffix(".json")?
                    .parse::<u64>()
                    .ok()
            })
            .collect();
        positions.sort_unstable();

        let start = start_position.map(|p| p + 1).unwrap_or(0);
        positions.retain(|&p| p >= start);
        Ok(positions)
    }

    /// Resolves a single QueryItem to a set of matching positions using indices.
    async fn get_positions_for_query_item(
        &self,
        index_manager: &IndexManager<S>,
        item: &crate::domain::QueryItem,
    ) -> Result<alloc::collections::BTreeSet<u64>, Error> {
        let mut event_type_positions: Option<alloc::collections::BTreeSet<u64>> = None;
        let mut tag_positions: Option<alloc::collections::BTreeSet<u64>> = None;

        // Event types: OR within (union)
        if !item.event_types.is_empty() {
            let type_strs: Vec<&str> = item.event_types.iter().map(|s| s.as_str()).collect();
            let positions = index_manager
                .get_positions_by_event_types_async("Indices", &type_strs)
                .await?;
            event_type_positions = Some(positions.into_iter().collect());
        }

        // Tags: AND within (intersection)
        if !item.tags.is_empty() {
            let mut tag_set: Option<alloc::collections::BTreeSet<u64>> = None;
            for tag in &item.tags {
                let positions = index_manager
                    .get_positions_by_tag_async("Indices", tag)
                    .await?;
                let pos_set: alloc::collections::BTreeSet<u64> = positions.into_iter().collect();
                tag_set = Some(match tag_set {
                    None => pos_set,
                    Some(existing) => existing.intersection(&pos_set).copied().collect(),
                });
            }
            tag_positions = tag_set;
        }

        // Combine: AND between event_types and tags
        match (event_type_positions, tag_positions) {
            (Some(et), Some(tg)) => Ok(et.intersection(&tg).copied().collect()),
            (Some(et), None) => Ok(et),
            (None, Some(tg)) => Ok(tg),
            (None, None) => Ok(alloc::collections::BTreeSet::new()),
        }
    }

    /// Resolves a Query to a sorted list of matching positions.
    async fn get_positions_for_query(
        &self,
        query: &Query,
        start_position: Option<u64>,
    ) -> Result<Vec<u64>, Error> {
        if query.items.is_empty() {
            return self.get_all_positions(start_position).await;
        }

        let index_manager = IndexManager::new(self.storage.clone());
        let mut all_positions = alloc::collections::BTreeSet::<u64>::new();

        for item in &query.items {
            let item_positions = self
                .get_positions_for_query_item(&index_manager, item)
                .await?;
            for pos in item_positions {
                all_positions.insert(pos);
            }
        }

        let start = start_position.map(|p| p + 1).unwrap_or(0);
        let result: Vec<u64> = all_positions.into_iter().filter(|&p| p >= start).collect();
        Ok(result)
    }

    async fn read_internal(
        &self,
        query: Query,
        start_position: Option<u64>,
        max_count: Option<usize>,
        options: Option<Vec<crate::domain::ReadOption>>,
    ) -> Result<Vec<EventRecord>, Error> {
        let mut positions = self.get_positions_for_query(&query, start_position).await?;

        if positions.is_empty() {
            return Ok(Vec::new());
        }

        let descending_opt = crate::domain::ReadOption::DESCENDING;
        let is_descending = options
            .as_ref()
            .is_some_and(|opts| opts.contains(&descending_opt));
        if is_descending {
            positions.reverse();
        }

        // For index-resolved queries, apply max_count before reading files
        if !query.items.is_empty() {
            if let Some(max) = max_count {
                positions.truncate(max);
            }
        }

        let dir_path = "Events";

        // For large batches where positions are already fully resolved (index-based
        // queries with max_count already applied), read files in parallel.
        let needs_post_filter = query.items.is_empty();
        if !needs_post_filter && positions.len() > 10 {
            let read_futures: Vec<_> = positions
                .iter()
                .map(|&pos| {
                    let storage = &self.storage;
                    async move {
                        let file_path = format!("{}/{:010}.json", dir_path, pos);
                        let data = storage.read_file(&file_path).await?;
                        serde_json::from_slice::<EventRecord>(&data)
                            .map_err(|_| Error::IoError)
                    }
                })
                .collect();
            let read_results = futures::future::join_all(read_futures).await;
            let mut results = Vec::with_capacity(read_results.len());
            for result in read_results {
                results.push(result?);
            }
            return Ok(results);
        }

        // Sequential path: used for small batches or Query::all() which needs
        // post-filtering and early termination via max_count.
        let mut results = Vec::with_capacity(positions.len());
        for pos in positions {
            let file_path = format!("{}/{:010}.json", dir_path, pos);
            let data = self.storage.read_file(&file_path).await?;
            let record =
                serde_json::from_slice::<EventRecord>(&data).map_err(|_| Error::IoError)?;

            if query.matches(&record) {
                results.push(record);
                if let Some(max) = max_count
                    && results.len() >= max
                {
                    break;
                }
            }
        }

        Ok(results)
    }
}

impl<S: StorageBackend + Send + Sync + Clone, C: Clock + Send + Sync> EventStore
    for MarmosaStore<S, C>
{
    async fn append_async(
        &self,
        events: Vec<EventData>,
        condition: Option<AppendCondition>,
    ) -> Result<(), Error> {
        let global_lock = "global_store";
        self.storage.acquire_stream_lock(global_lock).await?;

        let result = async {
            let dir_path = "Events";
            let _ = self.storage.create_dir_all(dir_path).await; // Ignore if exists

            let existing_files = self.storage.read_dir(dir_path).await.unwrap_or_default();
            let mut sequence = existing_files
                .iter()
                .filter_map(|f| {
                    f.split('/')
                        .next_back()?
                        .strip_suffix(".json")?
                        .parse::<u64>()
                        .ok()
                })
                .max()
                .map(|v| v + 1)
                .unwrap_or(0);

            if let Some(cond) = condition {
                let current_events = self
                    .read_internal(
                        cond.fail_if_events_match,
                        cond.after_sequence_position,
                        None,
                        None,
                    )
                    .await?;
                if !current_events.is_empty() {
                    return Err(Error::AppendConditionFailed);
                }
            }

            let timestamp = self.clock.now_millis();
            let index_manager = IndexManager::new(self.storage.clone());

            for event in events {
                let record = EventRecord {
                    position: sequence,
                    event_id: event.event_id,
                    event: event.event,
                    metadata: event.metadata,
                    timestamp,
                };

                let vec = serde_json::to_vec(&record).map_err(|_| Error::IoError)?;

                let file_path = format!("{}/{:010}.json", dir_path, sequence);
                self.storage.write_file(&file_path, &vec).await?;

                index_manager
                    .add_event_to_indices_async("Indices", &record)
                    .await?;

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
        self.read_internal(query, start_position, max_count, options)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::DomainEvent;
    use crate::ports::tests::{FakeClock, InMemoryStorage};

    #[tokio::test]
    async fn test_append_single_event_to_new_stream() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let clock = FakeClock::new(1696000000);
        let store = MarmosaStore::new(storage, clock);

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
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let clock = FakeClock::new(1696000000);
        let store = MarmosaStore::new(storage, clock);

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
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let clock = FakeClock::new(1696000000);
        let store = MarmosaStore::new(storage, clock);

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
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let clock = FakeClock::new(1696000000);
        let store = MarmosaStore::new(storage, clock);

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
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let clock = FakeClock::new(1696000000);
        let store = MarmosaStore::new(storage, clock);

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

        let result = store
            .read_async(Query::all(), None, None, None)
            .await
            .unwrap();
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
        let store = MarmosaStore::new(storage.clone(), clock);

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
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let clock = FakeClock::new(1696000000);
        let store = MarmosaStore::new(storage, clock);

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

        let result = store
            .read_async(Query::all(), None, None, None)
            .await
            .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].position, 0);
        assert_eq!(result[1].position, 1);
    }

    #[tokio::test]
    async fn test_read_async_with_query_all_returns_all_events() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let clock = FakeClock::new(1696000000);
        let store = MarmosaStore::new(storage, clock);

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

        let events = store
            .read_async(Query::all(), None, None, None)
            .await
            .unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].position, 0);
        assert_eq!(events[1].position, 1);
        assert_eq!(events[2].position, 2);
    }

    #[tokio::test]
    async fn test_read_async_with_single_event_type_returns_matching_events() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let clock = FakeClock::new(1696000000);
        let store = MarmosaStore::new(storage, clock);

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
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let clock = FakeClock::new(1696000000);
        let store = MarmosaStore::new(storage, clock);

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
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let clock = FakeClock::new(1696000000);
        let store = MarmosaStore::new(storage, clock);

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

        let events = store
            .read_async(Query::all(), Some(1), None, None)
            .await
            .unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].position, 2);
        assert_eq!(events[1].position, 3);
        assert_eq!(events[2].position, 4);
    }
}
pub mod maintenance;
