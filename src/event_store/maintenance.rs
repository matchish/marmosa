use crate::domain::{EventRecord, Query, QueryItem, Tag};
use crate::event_store::{EventStore, OpossumStore};
use crate::ports::{Clock, Error, StorageBackend};
use alloc::string::ToString;
use alloc::vec::Vec;

pub struct AddTagsResult {
    pub tags_added: usize,
    pub events_processed: usize,
}

pub trait EventStoreMaintenance {
    fn add_tags<F>(
        &self,
        event_type: &str,
        tag_factory: F,
    ) -> impl core::future::Future<Output = Result<AddTagsResult, Error>> + Send
    where
        F: Fn(&EventRecord) -> Vec<Tag> + Send + Sync;
}

impl<S: StorageBackend + Send + Sync, C: Clock + Send + Sync> EventStoreMaintenance
    for OpossumStore<S, C>
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
                let vec =
                    serde_json_core::to_vec::<_, 4096>(&record).map_err(|_| Error::IoError)?;
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
        let store = Arc::new(OpossumStore::new(storage.clone(), clock));

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
        let store = Arc::new(OpossumStore::new(storage.clone(), clock));

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
        let store = Arc::new(OpossumStore::new(storage.clone(), clock));

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
        let store = Arc::new(OpossumStore::new(storage.clone(), clock));

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
        let store = Arc::new(OpossumStore::new(storage.clone(), clock));

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
        let store = Arc::new(OpossumStore::new(storage.clone(), clock));

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
        let store = Arc::new(OpossumStore::new(storage.clone(), clock));

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
        let store = Arc::new(OpossumStore::new(storage.clone(), clock));

        let result = store.add_tags("", |_| alloc::vec![]).await;
        assert!(matches!(result, Err(Error::IoError)));
    }
}
