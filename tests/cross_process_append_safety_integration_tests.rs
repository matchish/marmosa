mod common;
use common::{FakeClock, InMemoryStorage};
use marmosa::domain::{DomainEvent, EventData, Query, Tag};
use marmosa::event_store::{EventStore, OpossumStore};
use std::sync::Arc;

fn create_store_from_storage(storage: Arc<InMemoryStorage>) -> Arc<OpossumStore<Arc<InMemoryStorage>, FakeClock>> {
    Arc::new(OpossumStore::new(storage, FakeClock::new(100)))
}

fn create_event(data: &str, source: &str) -> EventData {
    EventData {
        event_id: uuid::Uuid::new_v4().to_string(),
        event: DomainEvent {
            event_type: "CrossProcessTestEvent".to_string(),
            data: format!("{{\"Data\":\"{}\"}}", data),
            tags: vec![Tag { key: "source".to_string(), value: source.to_string() }],
        },
        metadata: None,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn two_instances_concurrent_appends_produce_contiguous_positions() {
    let storage = Arc::new(InMemoryStorage::new());
    let store1 = create_store_from_storage(Arc::clone(&storage));
    let store2 = create_store_from_storage(Arc::clone(&storage));

    let appends_per_instance = 50;
    let total_appends = appends_per_instance * 2;

    let mut tasks = vec![];
    for i in 0..appends_per_instance {
        let store = Arc::clone(&store1);
        tasks.push(tokio::spawn(async move {
            let evt = create_event(&format!("Instance1-{}", i), "instance1");
            store.append_async(vec![evt], None).await.unwrap();
        }));
    }

    for i in 0..appends_per_instance {
        let store = Arc::clone(&store2);
        tasks.push(tokio::spawn(async move {
            let evt = create_event(&format!("Instance2-{}", i), "instance2");
            store.append_async(vec![evt], None).await.unwrap();
        }));
    }

    futures::future::join_all(tasks).await;

    // Both instances share storage - read from either one
    let all_events = store1.read_async(Query::all(), None, None, None).await.unwrap();
    assert_eq!(all_events.len(), total_appends as usize);

    // Positions must be contiguous 1..100 with no gaps and no duplicates
    let mut positions: Vec<u64> = all_events.iter().map(|e| e.position).collect();
    positions.sort();

    let expected_positions: Vec<u64> = (0..(total_appends as u64)).collect();
    assert_eq!(positions, expected_positions);
}

#[tokio::test(flavor = "multi_thread")]
async fn two_instances_concurrent_appends_no_event_overwrite() {
    let storage = Arc::new(InMemoryStorage::new());
    let store1 = create_store_from_storage(Arc::clone(&storage));
    let store2 = create_store_from_storage(Arc::clone(&storage));

    let appends_per_instance = 50;

    let mut tasks = vec![];

    for i in 0..appends_per_instance {
        let store = Arc::clone(&store1);
        tasks.push(tokio::spawn(async move {
            let evt = create_event(&format!("Store1-Payload-{}", i), "store1");
            store.append_async(vec![evt], None).await.unwrap();
        }));
    }

    for i in 0..appends_per_instance {
        let store = Arc::clone(&store2);
        tasks.push(tokio::spawn(async move {
            let evt = create_event(&format!("Store2-Payload-{}", i), "store2");
            store.append_async(vec![evt], None).await.unwrap();
        }));
    }

    futures::future::join_all(tasks).await;

    let all_events = store1.read_async(Query::all(), None, None, None).await.unwrap();
    
    for i in 0..appends_per_instance {
        let payload1 = format!("{{\"Data\":\"Store1-Payload-{}\"}}", i);
        let found1 = all_events.iter().any(|e| e.event.data == payload1);
        assert!(found1, "Missing payload from store 1: {}", payload1);

        let payload2 = format!("{{\"Data\":\"Store2-Payload-{}\"}}", i);
        let found2 = all_events.iter().any(|e| e.event.data == payload2);
        assert!(found2, "Missing payload from store 2: {}", payload2);
    }
}
