mod common;

use common::{FakeClock, InMemoryStorage};
use marmosa::domain::{DomainEvent, EventData, Query, ReadOption};
use marmosa::event_store::{EventStore, MarmosaStore};
use std::sync::Arc;
use std::time::Instant;

fn create_store() -> Arc<MarmosaStore<Arc<InMemoryStorage>, FakeClock>> {
    let storage = Arc::new(InMemoryStorage::new());
    let clock = FakeClock::new(1000);
    Arc::new(MarmosaStore::new(storage, clock))
}

#[tokio::test]
async fn descending_order_should_be_fast_with_many_events_async() {
    let store = create_store();

    let mut events = Vec::new();
    for i in 0..500 {
        events.push(EventData {
            event_id: format!("test_event_{}", i),
            event: DomainEvent {
                event_type: "TestEvent".to_string(),
                data: format!(r#"{{"data": "Data{}"}}"#, i),
                tags: vec![],
            },
            metadata: None,
        });
    }

    store.append_async(events, None).await.unwrap();

    // Warm-up
    let _ = store
        .read_async(Query::all(), None, None, None)
        .await
        .unwrap();

    // Ascending
    let sw1 = Instant::now();
    let _ascending = store
        .read_async(Query::all(), None, None, None)
        .await
        .unwrap();
    let ascending_time = sw1.elapsed().as_micros();

    // Descending
    let sw2 = Instant::now();
    let descending = store
        .read_async(Query::all(), None, None, Some(vec![ReadOption::DESCENDING]))
        .await
        .unwrap();
    let descending_time = sw2.elapsed().as_micros();

    let ascending_time = std::cmp::max(ascending_time, 1);
    let ratio = descending_time as f64 / ascending_time as f64;

    assert!(
        ratio < 4.0,
        "Descending took {}us vs Ascending {}us (ratio: {:.2}x). Expected <4.0x overhead.",
        descending_time,
        ascending_time,
        ratio
    );

    assert_eq!(500, descending.len());
    let descending = descending;
    assert_eq!(499, descending[0].position); // Newest first
    assert_eq!(0, descending[499].position); // Oldest last
}
