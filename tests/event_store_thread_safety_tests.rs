mod common;

use common::{FakeClock, InMemoryStorage};
use marmosa::domain::{AppendCondition, DomainEvent, EventData, Query, QueryItem, Tag};
use marmosa::event_store::{EventStore, MarmosaStore};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

fn create_store() -> (
    Arc<InMemoryStorage>,
    Arc<MarmosaStore<Arc<InMemoryStorage>, FakeClock>>,
) {
    let storage = Arc::new(InMemoryStorage::new());
    let clock = FakeClock::new(1000);
    let store = Arc::new(MarmosaStore::new(Arc::clone(&storage), clock));
    (storage, store)
}

fn create_test_event(category: &str, data: &str, tags: Vec<Tag>) -> EventData {
    EventData {
        event_id: format!("{}-{}", category, data),
        event: DomainEvent {
            event_type: "ThreadSafetyTestEvent".to_string(),
            data: format!(r#"{{"Category": "{}", "Data": "{}"}}"#, category, data),
            tags,
        },
        metadata: None,
    }
}

#[tokio::test]
async fn concurrent_appends_serialized_execution_all_events_stored() {
    let (_, store) = create_store();
    let append_count = 50;
    let events_per_append = 5;

    let mut tasks = Vec::new();

    for i in 0..append_count {
        let store_clone = Arc::clone(&store);
        let task = tokio::spawn(async move {
            let mut events = Vec::new();
            for j in 0..events_per_append {
                events.push(create_test_event(
                    &format!("Append-{}", i),
                    &format!("Event-{}", j),
                    vec![Tag {
                        key: "appendId".to_string(),
                        value: i.to_string(),
                    }],
                ));
            }
            store_clone.append_async(events, None).await.unwrap();
        });
        tasks.push(task);
    }

    for task in tasks {
        task.await.unwrap();
    }

    let all_events = store
        .read_async(Query::all(), None, None, None)
        .await
        .unwrap();
    assert_eq!(all_events.len(), append_count * events_per_append);

    let mut positions: Vec<u64> = all_events.iter().map(|e| e.position).collect();
    positions.sort();

    let expected_positions: Vec<u64> = (0..(append_count * events_per_append) as u64).collect();
    assert_eq!(positions, expected_positions);

    let mut unique_positions = positions.clone();
    unique_positions.dedup();
    assert_eq!(positions.len(), unique_positions.len());
}

#[tokio::test]
async fn concurrent_reads_during_writes_eventually_consistent() {
    let (_, store) = create_store();
    let write_count = 100;
    let read_count = 50;

    let write_tasks_completed = Arc::new(AtomicUsize::new(0));
    let mut write_tasks = Vec::new();

    for i in 1..=write_count {
        let store_clone = Arc::clone(&store);
        let completed_counter = Arc::clone(&write_tasks_completed);
        let task = tokio::spawn(async move {
            let evt = create_test_event(
                "Write",
                &format!("Event-{}", i),
                vec![Tag {
                    key: "sequence".to_string(),
                    value: i.to_string(),
                }],
            );
            store_clone.append_async(vec![evt], None).await.unwrap();
            completed_counter.fetch_add(1, Ordering::SeqCst);
        });
        write_tasks.push(task);
    }

    let mut read_tasks = Vec::new();
    for _ in 0..read_count {
        let store_clone = Arc::clone(&store);
        let completed_counter = Arc::clone(&write_tasks_completed);
        let task = tokio::spawn(async move {
            let mut local_results = Vec::new();
            while completed_counter.load(Ordering::SeqCst) < write_count {
                let events = store_clone
                    .read_async(Query::all(), None, None, None)
                    .await
                    .unwrap();
                local_results.push(events.len());
                // tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
            }
            local_results
        });
        read_tasks.push(task);
    }

    for task in write_tasks {
        task.await.unwrap();
    }

    let mut all_read_results = Vec::new();
    for task in read_tasks {
        let results = task.await.unwrap();
        all_read_results.extend(results);
    }

    let final_events = store
        .read_async(Query::all(), None, None, None)
        .await
        .unwrap();
    assert_eq!(final_events.len(), write_count);

    all_read_results.sort();
    if !all_read_results.is_empty() {
        assert!(*all_read_results.first().unwrap() > 0);
        assert!(*all_read_results.last().unwrap() <= write_count);
    }
}

#[tokio::test]
async fn concurrent_appends_with_optimistic_concurrency_detects_conflicts() {
    let (_, store) = create_store();
    let resource_id = "res-uuid-123";

    let initial_evt = create_test_event(
        "Resource",
        "Created",
        vec![Tag {
            key: "resourceId".to_string(),
            value: resource_id.to_string(),
        }],
    );
    store.append_async(vec![initial_evt], None).await.unwrap();

    let query = Query {
        items: vec![QueryItem {
            event_types: vec![],
            tags: vec![Tag {
                key: "resourceId".to_string(),
                value: resource_id.to_string(),
            }],
        }],
    };

    let initial_events = store
        .read_async(query.clone(), None, None, None)
        .await
        .unwrap();
    let initial_position = initial_events.last().unwrap().position;

    let success_count = Arc::new(AtomicUsize::new(0));
    let failure_count = Arc::new(AtomicUsize::new(0));

    let mut tasks = Vec::new();
    for i in 0..10 {
        let store_clone = Arc::clone(&store);
        let succ_clone = Arc::clone(&success_count);
        let fail_clone = Arc::clone(&failure_count);

        let task = tokio::spawn(async move {
            let update_evt = create_test_event(
                "Resource",
                &format!("Updated-{}", i),
                vec![Tag {
                    key: "resourceId".to_string(),
                    value: "res-uuid-123".to_string(),
                }],
            );

            let condition = AppendCondition {
                after_sequence_position: Some(initial_position),
                fail_if_events_match: Query::all(), // Condition checking reads current state
            };

            match store_clone
                .append_async(vec![update_evt], Some(condition))
                .await
            {
                Ok(_) => {
                    succ_clone.fetch_add(1, Ordering::SeqCst);
                }
                Err(_) => {
                    fail_clone.fetch_add(1, Ordering::SeqCst);
                }
            }
        });
        tasks.push(task);
    }

    for task in tasks {
        task.await.unwrap();
    }

    assert_eq!(success_count.load(Ordering::SeqCst), 1);
    assert_eq!(failure_count.load(Ordering::SeqCst), 9);

    let final_events = store.read_async(query, None, None, None).await.unwrap();
    assert_eq!(final_events.len(), 2); // Initial + one update
}

#[tokio::test]
async fn stress_test_high_concurrent_load_maintains_integrity() {
    let (_, store) = create_store();
    let entity_count = 20;
    let events_per_entity = 50;
    let total_events = entity_count * events_per_entity;

    let mut tasks = Vec::new();

    for entity_id in 0..entity_count {
        for event_num in 0..events_per_entity {
            let store_clone = Arc::clone(&store);
            let task = tokio::spawn(async move {
                let evt = create_test_event(
                    &format!("Entity-{}", entity_id),
                    &format!("Event-{}", event_num),
                    vec![Tag {
                        key: "entityId".to_string(),
                        value: entity_id.to_string(),
                    }],
                );
                store_clone.append_async(vec![evt], None).await.unwrap();
            });
            tasks.push(task);
        }
    }

    for task in tasks {
        task.await.unwrap();
    }

    let all_events = store
        .read_async(Query::all(), None, None, None)
        .await
        .unwrap();
    assert_eq!(all_events.len(), total_events);

    let mut positions: Vec<u64> = all_events.iter().map(|e| e.position).collect();
    positions.sort();
    let expected_positions: Vec<u64> = (0..total_events as u64).collect();
    assert_eq!(positions, expected_positions);

    for i in 0..entity_count {
        let q = Query {
            items: vec![QueryItem {
                event_types: vec![],
                tags: vec![Tag {
                    key: "entityId".to_string(),
                    value: i.to_string(),
                }],
            }],
        };
        let entity_events = store.read_async(q, None, None, None).await.unwrap();
        assert_eq!(entity_events.len(), events_per_entity);
    }
}

#[tokio::test]
async fn concurrent_appends_to_multiple_contexts_isolated() {
    // Tests thread concurrency per context logic or general interleaving
    let (_, store) = create_store();
    let events_per_context = 50;

    let mut tasks = Vec::new();

    for i in 0..events_per_context {
        let store_clone = Arc::clone(&store);
        let task = tokio::spawn(async move {
            let evt = create_test_event(
                "Context1",
                &format!("Event-{}", i),
                vec![Tag {
                    key: "context".to_string(),
                    value: "context1".to_string(),
                }],
            );
            store_clone.append_async(vec![evt], None).await.unwrap();
        });
        tasks.push(task);
    }

    for i in 0..events_per_context {
        let store_clone = Arc::clone(&store);
        let task = tokio::spawn(async move {
            let evt = create_test_event(
                "Context2",
                &format!("Event-{}", i),
                vec![Tag {
                    key: "context".to_string(),
                    value: "context2".to_string(),
                }],
            );
            store_clone.append_async(vec![evt], None).await.unwrap();
        });
        tasks.push(task);
    }

    for task in tasks {
        task.await.unwrap();
    }

    let q1 = Query {
        items: vec![QueryItem {
            event_types: vec![],
            tags: vec![Tag {
                key: "context".to_string(),
                value: "context1".to_string(),
            }],
        }],
    };
    let ctx1_events = store.read_async(q1, None, None, None).await.unwrap();

    let q2 = Query {
        items: vec![QueryItem {
            event_types: vec![],
            tags: vec![Tag {
                key: "context".to_string(),
                value: "context2".to_string(),
            }],
        }],
    };
    let ctx2_events = store.read_async(q2, None, None, None).await.unwrap();

    assert_eq!(ctx1_events.len(), events_per_context);
    assert_eq!(ctx2_events.len(), events_per_context);
}

#[tokio::test]
async fn concurrent_query_by_event_type_returns_consistent_results() {
    let (_, store) = create_store();
    let event_count = 100;

    let mut add_tasks = Vec::new();
    for i in 0..event_count {
        let store_clone = Arc::clone(&store);
        let task = tokio::spawn(async move {
            let evt = create_test_event("Test", &format!("Data-{}", i), vec![]);
            store_clone.append_async(vec![evt], None).await.unwrap();
        });
        add_tasks.push(task);
    }

    for task in add_tasks {
        task.await.unwrap();
    }

    let mut query_tasks = Vec::new();
    for _ in 0..20 {
        let store_clone = Arc::clone(&store);
        let task = tokio::spawn(async move {
            let q = Query {
                items: vec![QueryItem {
                    event_types: vec!["ThreadSafetyTestEvent".to_string()],
                    tags: vec![],
                }],
            };
            store_clone
                .read_async(q, None, None, None)
                .await
                .unwrap()
                .len()
        });
        query_tasks.push(task);
    }

    for task in query_tasks {
        let count = task.await.unwrap();
        assert_eq!(count, event_count);
    }
}

#[tokio::test]
async fn concurrent_append_with_fail_if_match_serializes_correctly() {
    let (_, store) = create_store();
    let user_id = "user-123";
    let successful_appends = Arc::new(AtomicUsize::new(0));

    let mut tasks = Vec::new();
    for i in 0..20 {
        let store_clone = Arc::clone(&store);
        let succ_clone = Arc::clone(&successful_appends);
        let task = tokio::spawn(async move {
            let evt = create_test_event(
                "FirstLogin",
                &format!("Attempt-{}", i),
                vec![
                    Tag {
                        key: "userId".to_string(),
                        value: user_id.to_string(),
                    },
                    Tag {
                        key: "type".to_string(),
                        value: "FirstLogin".to_string(),
                    },
                ],
            );

            let condition = AppendCondition {
                after_sequence_position: None,
                fail_if_events_match: Query {
                    items: vec![QueryItem {
                        event_types: vec![],
                        tags: vec![
                            Tag {
                                key: "userId".to_string(),
                                value: user_id.to_string(),
                            },
                            Tag {
                                key: "type".to_string(),
                                value: "FirstLogin".to_string(),
                            },
                        ],
                    }],
                },
            };

            if store_clone
                .append_async(vec![evt], Some(condition))
                .await
                .is_ok()
            {
                succ_clone.fetch_add(1, Ordering::SeqCst);
            }
        });
        tasks.push(task);
    }

    for task in tasks {
        task.await.unwrap();
    }

    assert_eq!(successful_appends.load(Ordering::SeqCst), 1);

    let q = Query {
        items: vec![QueryItem {
            event_types: vec![],
            tags: vec![
                Tag {
                    key: "userId".to_string(),
                    value: user_id.to_string(),
                },
                Tag {
                    key: "type".to_string(),
                    value: "FirstLogin".to_string(),
                },
            ],
        }],
    };
    let first_login_events = store.read_async(q, None, None, None).await.unwrap();
    assert_eq!(first_login_events.len(), 1);
}
