//! Event tag maintenance operations for existing events.
//!
//! # Overview
//!
//! This module provides additive tag migration on top of the event store.
//! [`EventStoreMaintenance::add_tags`] scans events of a specific type and writes only tags whose
//! key does not already exist on an event.
//!
//! The operation is intentionally additive:
//! - existing tag keys are preserved,
//! - no tag removal is performed,
//! - no in-place key replacement is performed.
//!
//! This keeps event enrichment predictable while allowing read-side indices to evolve over time.

use crate::domain::{EventRecord, Query, QueryItem, Tag};
use crate::event_store::{EventStore, MarmosaStore};
use crate::ports::{Clock, Error, StorageBackend};
use alloc::string::ToString;
use alloc::vec::Vec;

/// Result summary returned by [`EventStoreMaintenance::add_tags`].
///
/// # Overview
///
/// - `events_processed`: number of matching events scanned.
/// - `tags_added`: number of new tag entries actually persisted.
///
/// # Examples
///
/// ```rust
/// use marmosa::event_store::maintenance::AddTagsResult;
///
/// let result = AddTagsResult {
///     tags_added: 3,
///     events_processed: 10,
/// };
///
/// assert_eq!(result.tags_added, 3);
/// assert_eq!(result.events_processed, 10);
/// assert!(result.events_processed >= result.tags_added);
/// ```
pub struct AddTagsResult {
    pub tags_added: usize,
    pub events_processed: usize,
}

/// Additive tag maintenance API for event stores.
///
/// # Overview
///
/// Implementations enrich events of one `event_type` by invoking a `tag_factory` for each matched
/// [`EventRecord`]. Returned tags are merged by key: if a key already exists on the event, that
/// key is left unchanged.
///
/// # Examples
///
/// ```rust,no_run
/// use marmosa::domain::{EventRecord, Tag};
/// use marmosa::event_store::maintenance::EventStoreMaintenance;
/// use marmosa::ports::Error;
///
/// async fn add_region_tags<S: EventStoreMaintenance + ?Sized>(store: &S) -> Result<(), Error> {
///     let result = store
///         .add_tags("CourseCreated", |record: &EventRecord| {
///             if record.event.tags.iter().any(|t| t.key == "courseId") {
///                 vec![Tag {
///                     key: "region".to_string(),
///                     value: "EU".to_string(),
///                 }]
///             } else {
///                 vec![]
///             }
///         })
///         .await?;
///
///     assert!(result.events_processed >= result.tags_added);
///     Ok(())
/// }
/// ```
pub trait EventStoreMaintenance {
    /// Adds derived tags to all events of `event_type`.
    ///
    /// # Notes
    ///
    /// - Additive-only behavior: existing keys are not overwritten.
    /// - `tag_factory` receives the full persisted [`EventRecord`], including position and
    ///   existing tags.
    ///
    /// # Errors
    ///
    /// Returns [`Error::IoError`] when persistence or serialization fails, or when `event_type`
    /// is empty.
    fn add_tags<F>(
        &self,
        event_type: &str,
        tag_factory: F,
    ) -> impl core::future::Future<Output = Result<AddTagsResult, Error>> + Send
    where
        F: Fn(&EventRecord) -> Vec<Tag> + Send + Sync;
}

impl<S: StorageBackend + Send + Sync, C: Clock + Send + Sync> EventStoreMaintenance
    for MarmosaStore<S, C>
{
    async fn add_tags<F>(&self, event_type: &str, tag_factory: F) -> Result<AddTagsResult, Error>
    where
        F: Fn(&EventRecord) -> Vec<Tag> + Send + Sync,
    {
        if event_type.is_empty() {
            return Err(Error::IoError); // mapped to ArgumentException
        }

        let mut events_processed = 0;
        let mut tags_added = 0;

        let query = Query {
            items: alloc::vec![QueryItem {
                event_types: alloc::vec![event_type.to_string()],
                tags: alloc::vec![],
            }],
        };

        let events = self.read_async(query, None, None, None).await?;

        for mut record in events {
            events_processed += 1;

            let new_tags = tag_factory(&record);
            let mut added_for_this_event = 0;

            for tag in new_tags {
                if !record.event.tags.iter().any(|t| t.key == tag.key) {
                    record.event.tags.push(tag.clone());
                    added_for_this_event += 1;

                    // Add index
                    let index_dir = alloc::format!(
                        "Indices/{}/{}",
                        tag.key.to_lowercase(),
                        tag.value.to_lowercase()
                    );
                    let _ = self.storage.create_dir_all(&index_dir).await;
                    let index_file = alloc::format!("{}/{:010}.json", index_dir, record.position);
                    let _ = self.storage.write_file(&index_file, b"{}").await;
                }
            }

            if added_for_this_event > 0 {
                tags_added += added_for_this_event;

                // Rewrite event to file
                let vec = serde_json::to_vec(&record).map_err(|_| Error::IoError)?;
                let file_path = alloc::format!("Events/{:010}.json", record.position);
                let _ = self.storage.write_file(&file_path, &vec).await;
            }
        }

        Ok(AddTagsResult {
            tags_added,
            events_processed,
        })
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
    async fn add_tags_adds_new_tags_to_all_matching_events() {
        let storage = Arc::new(InMemoryStorage::new());
        let clock = FakeClock::new(100);
        let store = Arc::new(MarmosaStore::new(storage.clone(), clock));

        store
            .append_async(
                alloc::vec![
                    create_test_event("CourseCreated"),
                    create_test_event("CourseCreated"),
                ],
                None,
            )
            .await
            .unwrap();

        let result = store
            .add_tags("CourseCreated", |_| {
                alloc::vec![Tag {
                    key: "region".to_string(),
                    value: "EU".to_string()
                }]
            })
            .await
            .unwrap();

        assert_eq!(result.tags_added, 2);
        assert_eq!(result.events_processed, 2);

        let events = store
            .read_async(Query::all(), None, None, None)
            .await
            .unwrap();
        for event in events {
            assert!(
                event
                    .event
                    .tags
                    .iter()
                    .any(|t| t.key == "region" && t.value == "EU")
            );
        }
    }

    #[tokio::test]
    async fn add_tags_only_affects_events_of_specified_type() {
        let storage = Arc::new(InMemoryStorage::new());
        let clock = FakeClock::new(100);
        let store = Arc::new(MarmosaStore::new(storage.clone(), clock));

        store
            .append_async(
                alloc::vec![
                    create_test_event("CourseCreated"),
                    create_test_event("StudentRegistered"),
                    create_test_event("CourseCreated"),
                ],
                None,
            )
            .await
            .unwrap();

        store
            .add_tags("CourseCreated", |_| {
                alloc::vec![Tag {
                    key: "region".to_string(),
                    value: "EU".to_string()
                }]
            })
            .await
            .unwrap();

        let events = store
            .read_async(Query::all(), None, None, None)
            .await
            .unwrap();
        let course_created = events
            .iter()
            .filter(|e| e.event.event_type == "CourseCreated")
            .collect::<Vec<_>>();
        let student = events
            .iter()
            .filter(|e| e.event.event_type == "StudentRegistered")
            .collect::<Vec<_>>();

        for e in course_created {
            assert!(e.event.tags.iter().any(|t| t.key == "region"));
        }
        for e in student {
            assert!(!e.event.tags.iter().any(|t| t.key == "region"));
        }
    }

    #[tokio::test]
    async fn add_tags_skips_tags_whose_key_already_exists() {
        let storage = Arc::new(InMemoryStorage::new());
        let clock = FakeClock::new(100);
        let store = Arc::new(MarmosaStore::new(storage.clone(), clock));

        let mut evt = create_test_event("CourseCreated");
        evt.event.tags.push(Tag {
            key: "region".to_string(),
            value: "US".to_string(),
        });
        store.append_async(alloc::vec![evt], None).await.unwrap();

        let result = store
            .add_tags("CourseCreated", |_| {
                alloc::vec![Tag {
                    key: "region".to_string(),
                    value: "EU".to_string()
                }]
            })
            .await
            .unwrap();

        assert_eq!(result.tags_added, 0);
        assert_eq!(result.events_processed, 1);

        let events = store
            .read_async(Query::all(), None, None, None)
            .await
            .unwrap();
        assert_eq!(events[0].event.tags.len(), 1);
        assert_eq!(events[0].event.tags[0].value, "US");
    }

    #[tokio::test]
    async fn add_tags_adds_only_new_keys_when_event_has_some_tags() {
        let storage = Arc::new(InMemoryStorage::new());
        let clock = FakeClock::new(100);
        let store = Arc::new(MarmosaStore::new(storage.clone(), clock));

        let mut evt = create_test_event("CourseCreated");
        evt.event.tags.push(Tag {
            key: "courseId".to_string(),
            value: "abc".to_string(),
        });
        store.append_async(alloc::vec![evt], None).await.unwrap();

        let result = store
            .add_tags("CourseCreated", |_| {
                alloc::vec![
                    Tag {
                        key: "courseId".to_string(),
                        value: "xyz".to_string()
                    },
                    Tag {
                        key: "region".to_string(),
                        value: "EU".to_string()
                    }
                ]
            })
            .await
            .unwrap();

        assert_eq!(result.tags_added, 1);

        let events = store
            .read_async(Query::all(), None, None, None)
            .await
            .unwrap();
        let tags = &events[0].event.tags;
        assert_eq!(tags.len(), 2);
        assert_eq!(
            tags.iter().find(|t| t.key == "courseId").unwrap().value,
            "abc"
        );
        assert_eq!(tags.iter().find(|t| t.key == "region").unwrap().value, "EU");
    }

    #[tokio::test]
    async fn add_tags_returns_zero_when_no_events_of_type() {
        let storage = Arc::new(InMemoryStorage::new());
        let clock = FakeClock::new(100);
        let store = Arc::new(MarmosaStore::new(storage.clone(), clock));

        let result = store
            .add_tags("NonExistentEventType", |_| {
                alloc::vec![Tag {
                    key: "key".to_string(),
                    value: "value".to_string()
                }]
            })
            .await
            .unwrap();

        assert_eq!(result.tags_added, 0);
        assert_eq!(result.events_processed, 0);
    }

    #[tokio::test]
    async fn add_tags_updates_tag_index_so_new_tag_query_finds_events() {
        let storage = Arc::new(InMemoryStorage::new());
        let clock = FakeClock::new(100);
        let store = Arc::new(MarmosaStore::new(storage.clone(), clock));

        store
            .append_async(alloc::vec![create_test_event("CourseCreated")], None)
            .await
            .unwrap();

        store
            .add_tags("CourseCreated", |_| {
                alloc::vec![Tag {
                    key: "region".to_string(),
                    value: "EU".to_string()
                }]
            })
            .await
            .unwrap();

        let query = Query {
            items: alloc::vec![QueryItem {
                event_types: alloc::vec![],
                tags: alloc::vec![Tag {
                    key: "region".to_string(),
                    value: "EU".to_string()
                }],
            }],
        };

        let events = store.read_async(query, None, None, None).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event.event_type, "CourseCreated");
    }

    #[tokio::test]
    async fn add_tags_tag_factory_receives_full_sequenced_event() {
        let storage = Arc::new(InMemoryStorage::new());
        let clock = FakeClock::new(100);
        let store = Arc::new(MarmosaStore::new(storage.clone(), clock));

        let mut evt = create_test_event("CourseCreated");
        evt.event.tags.push(Tag {
            key: "courseId".to_string(),
            value: "course-42".to_string(),
        });
        store.append_async(alloc::vec![evt], None).await.unwrap();

        store
            .add_tags("CourseCreated", |seq_evt| {
                if let Some(course_id) = seq_evt.event.tags.iter().find(|t| t.key == "courseId") {
                    alloc::vec![Tag {
                        key: "derived".to_string(),
                        value: alloc::format!("from-{}", course_id.value)
                    }]
                } else {
                    alloc::vec![]
                }
            })
            .await
            .unwrap();

        let events = store
            .read_async(Query::all(), None, None, None)
            .await
            .unwrap();
        assert!(
            events[0]
                .event
                .tags
                .iter()
                .any(|t| t.key == "derived" && t.value == "from-course-42")
        );
    }

    #[tokio::test]
    async fn add_tags_with_empty_event_type_returns_error() {
        let storage = Arc::new(InMemoryStorage::new());
        let clock = FakeClock::new(100);
        let store = Arc::new(MarmosaStore::new(storage.clone(), clock));

        let result = store.add_tags("", |_| alloc::vec![]).await;
        assert!(matches!(result, Err(Error::IoError)));
    }
}
