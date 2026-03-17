mod common;

use common::{FakeClock, InMemoryStorage};
use marmosa::domain::{DomainEvent, EventData, Query, QueryItem, Tag};
use marmosa::event_store::{EventStore, OpossumStore};
use marmosa::extensions::EventStoreExt;
use std::sync::Arc;

fn create_store() -> (
    Arc<InMemoryStorage>,
    OpossumStore<Arc<InMemoryStorage>, FakeClock>,
) {
    let storage = Arc::new(InMemoryStorage::new());
    let clock = FakeClock::new(1000);
    let store = OpossumStore::new(Arc::clone(&storage), clock);
    (storage, store)
}

async fn append_order_async(store: &impl EventStoreExt, order_id: &str) {
    let event = EventData {
        event_id: format!("order-{}", order_id),
        event: DomainEvent {
            event_type: "OrderEvent".to_string(),
            data: format!(r#"{{"OrderId": "{}"}}"#, order_id),
            tags: vec![Tag {
                key: "orderId".to_string(),
                value: order_id.to_string(),
            }],
        },
        metadata: None,
    };
    store.append_single_async(event, None).await.unwrap();
}

async fn append_shipment_async(store: &impl EventStoreExt, shipment_id: &str) {
    let event = EventData {
        event_id: format!("shipment-{}", shipment_id),
        event: DomainEvent {
            event_type: "ShipmentEvent".to_string(),
            data: format!(r#"{{"ShipmentId": "{}"}}"#, shipment_id),
            tags: vec![Tag {
                key: "shipmentId".to_string(),
                value: shipment_id.to_string(),
            }],
        },
        metadata: None,
    };
    store.append_single_async(event, None).await.unwrap();
}

#[tokio::test]
async fn read_async_without_from_position_returns_all_events() {
    let (_, store) = create_store();
    append_order_async(&store, "o1").await; // pos 0
    append_order_async(&store, "o2").await; // pos 1
    append_order_async(&store, "o3").await; // pos 2

    let events = store
        .read_async(Query::all(), None, None, None)
        .await
        .unwrap();
    assert_eq!(events.len(), 3);
}

#[tokio::test]
async fn read_async_with_from_position_returns_only_events_after_that_position() {
    let (_, store) = create_store();
    for i in 1..=5 {
        append_order_async(&store, &format!("o{}", i)).await; // 0, 1, 2, 3, 4
    }

    // Request only events after position 2 (should return 3 and 4)
    let events = store
        .read_async(Query::all(), Some(2), None, None)
        .await
        .unwrap();

    assert_eq!(events.len(), 2);
    assert_eq!(events[0].position, 3);
    assert_eq!(events[1].position, 4); // C# has positions 4 and 5 if 1..=5 created 5 items. 
    // Wait: C# adds positions 0,1,2,3,4.
    // After 2 -> positions 3 and 4. Correct.
}

#[tokio::test]
async fn read_async_with_from_position_at_last_event_returns_empty() {
    let (_, store) = create_store();
    append_order_async(&store, "o1").await;
    append_order_async(&store, "o2").await;
    append_order_async(&store, "o3").await;

    let all_events = store
        .read_async(Query::all(), None, None, None)
        .await
        .unwrap();
    let last_position = all_events.iter().map(|e| e.position).max().unwrap();

    let events = store
        .read_async(Query::all(), Some(last_position), None, None)
        .await
        .unwrap();
    assert!(events.is_empty());
}

#[tokio::test]
async fn read_async_with_from_position_and_event_type_filter_returns_only_matching_types_after_position()
 {
    let (_, store) = create_store();
    append_order_async(&store, "o1").await; // pos 0
    append_shipment_async(&store, "s1").await; // pos 1
    append_order_async(&store, "o2").await; // pos 2
    append_shipment_async(&store, "s2").await; // pos 3
    append_order_async(&store, "o3").await; // pos 4

    let query = Query {
        items: vec![QueryItem {
            event_types: vec!["OrderEvent".to_string()],
            tags: vec![],
        }],
    };

    // After pos 1, we only want OrderEvents -> should be at pos 2 and 4
    let events = store.read_async(query, Some(1), None, None).await.unwrap();

    assert_eq!(events.len(), 2);
    assert_eq!(events[0].position, 2);
    assert_eq!(events[1].position, 4);
    assert!(events.iter().all(|e| e.event.event_type == "OrderEvent"));
}

#[tokio::test]
async fn read_async_with_from_position_and_tag_filter_returns_only_matching_tags_after_position() {
    let (_, store) = create_store();
    let tag = Tag {
        key: "orderId".to_string(),
        value: "o-special".to_string(),
    };

    append_order_async(&store, "o-special").await; // pos 0
    append_order_async(&store, "o-other").await; // pos 1
    append_order_async(&store, "o-special").await; // pos 2
    append_order_async(&store, "o-special").await; // pos 3

    let query = Query {
        items: vec![QueryItem {
            event_types: vec![],
            tags: vec![tag],
        }],
    };

    // After pos 0 -> should return pos 2 and 3
    let events = store.read_async(query, Some(0), None, None).await.unwrap();

    assert_eq!(events.len(), 2);
    assert_eq!(events[0].position, 2);
    assert_eq!(events[1].position, 3);
}

#[tokio::test]
async fn read_async_can_poll_incrementally_simulating_projection_daemon() {
    let (_, store) = create_store();

    // First batch
    append_order_async(&store, "o1").await; // pos 0
    append_order_async(&store, "o2").await; // pos 1

    let first_batch = store
        .read_async(Query::all(), None, None, None)
        .await
        .unwrap();
    assert_eq!(first_batch.len(), 2);

    let checkpoint = first_batch.iter().map(|e| e.position).max().unwrap_or(0); // 1

    // New events
    append_order_async(&store, "o3").await; // pos 2
    append_order_async(&store, "o4").await; // pos 3

    let second_batch = store
        .read_async(Query::all(), Some(checkpoint), None, None)
        .await
        .unwrap();
    assert_eq!(second_batch.len(), 2);
    assert_eq!(second_batch[0].position, 2);
    assert_eq!(second_batch[1].position, 3);
}

#[tokio::test]
async fn read_async_with_from_position_extension_method_returns_only_events_after_position() {
    let (_, store) = create_store();
    append_order_async(&store, "o1").await; // pos 0
    append_order_async(&store, "o2").await; // pos 1
    append_order_async(&store, "o3").await; // pos 2

    let events = store
        .read_async(Query::all(), Some(0), None, None)
        .await
        .unwrap();

    assert_eq!(events.len(), 2);
    assert_eq!(events[0].position, 1);
    assert_eq!(events[1].position, 2);
}
