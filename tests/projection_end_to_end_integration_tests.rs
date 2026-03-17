mod common;

use common::{FakeClock, InMemoryStorage};
use marmosa::domain::{DomainEvent, EventData, EventRecord, Query, QueryItem, Tag};
use marmosa::event_store::{EventStore, OpossumStore};
use marmosa::projections::{
    ProjectionDefinition, ProjectionRunner, ProjectionStore, StorageBackendProjectionStore,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct E2EOrderItem {
    pub product_name: String,
    pub price: f64,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct E2EOrderState {
    pub order_id: String,
    pub customer_name: String,
    pub customer_email: String,
    pub total_amount: f64,
    pub item_count: i32,
    pub items: Vec<E2EOrderItem>,
}

pub struct E2EOrderProjection;

impl ProjectionDefinition for E2EOrderProjection {
    type State = E2EOrderState;

    fn projection_name(&self) -> &str {
        "E2EOrders"
    }

    fn event_types(&self) -> Query {
        Query {
            items: vec![QueryItem {
                event_types: vec![
                    "E2EOrderCreatedEvent".to_string(),
                    "E2EItemAddedEvent".to_string(),
                    "E2EOrderCancelledEvent".to_string(),
                ],
                tags: vec![],
            }],
        }
    }

    fn key_selector(&self, event: &EventRecord) -> Option<String> {
        event
            .event
            .tags
            .iter()
            .find(|t| t.key == "orderId")
            .map(|t| t.value.clone())
    }

    fn apply(&self, state: Option<Self::State>, event: &EventRecord) -> Option<Self::State> {
        match event.event.event_type.as_str() {
            "E2EOrderCreatedEvent" => {
                #[derive(Deserialize)]
                struct Created {
                    order_id: String,
                    customer_name: String,
                    customer_email: String,
                }
                if let Ok(data) = serde_json::from_str::<Created>(
                    event.event.data.replace("\\\"", "\"").trim_matches('"'),
                ) {
                    Some(E2EOrderState {
                        order_id: data.order_id,
                        customer_name: data.customer_name,
                        customer_email: data.customer_email,
                        total_amount: 0.0,
                        item_count: 0,
                        items: vec![],
                    })
                } else {
                    state
                }
            }
            "E2EItemAddedEvent" => {
                if let Some(mut s) = state {
                    #[derive(Deserialize)]
                    struct ItemAdded {
                        product_name: String,
                        price: f64,
                    }
                    if let Ok(data) = serde_json::from_str::<ItemAdded>(
                        event.event.data.replace("\\\"", "\"").trim_matches('"'),
                    ) {
                        s.total_amount += data.price;
                        s.item_count += 1;
                        s.items.push(E2EOrderItem {
                            product_name: data.product_name,
                            price: data.price,
                        });
                    }
                    Some(s)
                } else {
                    None
                }
            }
            "E2EOrderCancelledEvent" => None,
            _ => state,
        }
    }
}

#[tokio::test]
async fn end_to_end_create_and_query_order_works_correctly() {
    let storage = Arc::new(InMemoryStorage::new());
    let store = OpossumStore::new(Arc::clone(&storage), FakeClock::new(1000));
    let projection_store =
        StorageBackendProjectionStore::new(Arc::clone(&storage), "E2EOrders".to_string());
    let runner = ProjectionRunner::new(Arc::clone(&storage), E2EOrderProjection, projection_store);

    let order_id = "test-order-1";

    let events = vec![
        EventData {
            event_id: uuid::Uuid::new_v4().to_string(),
            event: DomainEvent {
                event_type: "E2EOrderCreatedEvent".to_string(),
                data: serde_json::to_string(&serde_json::json!({
                    "order_id": order_id,
                    "customer_name": "Customer 1",
                    "customer_email": "customer1@test.com"
                }))
                .unwrap(),
                tags: vec![Tag {
                    key: "orderId".to_string(),
                    value: order_id.to_string(),
                }],
            },
            metadata: None,
        },
        EventData {
            event_id: uuid::Uuid::new_v4().to_string(),
            event: DomainEvent {
                event_type: "E2EItemAddedEvent".to_string(),
                data: serde_json::to_string(&serde_json::json!({
                    "product_name": "Product A",
                    "price": 99.99
                }))
                .unwrap(),
                tags: vec![Tag {
                    key: "orderId".to_string(),
                    value: order_id.to_string(),
                }],
            },
            metadata: None,
        },
        EventData {
            event_id: uuid::Uuid::new_v4().to_string(),
            event: DomainEvent {
                event_type: "E2EItemAddedEvent".to_string(),
                data: serde_json::to_string(&serde_json::json!({
                    "product_name": "Product B",
                    "price": 49.99
                }))
                .unwrap(),
                tags: vec![Tag {
                    key: "orderId".to_string(),
                    value: order_id.to_string(),
                }],
            },
            metadata: None,
        },
    ];

    store.append_async(events, None).await.unwrap();

    let all_events = store
        .read_async(Query::all(), None, None, None)
        .await
        .unwrap();
    runner.process_events(&all_events).await.unwrap();

    let p_store = StorageBackendProjectionStore::new(Arc::clone(&storage), "E2EOrders".to_string());
    let order: Option<E2EOrderState> = p_store.get(order_id).await.unwrap();

    assert!(order.is_some());
    let order = order.unwrap();
    assert_eq!(order.order_id, order_id);
    assert_eq!(order.customer_name, "Customer 1");
    assert_eq!(order.customer_email, "customer1@test.com");
    assert_eq!(order.total_amount, 149.98);
    assert_eq!(order.item_count, 2);
    assert_eq!(order.items.len(), 2);
    assert!(order.items.iter().any(|i| i.product_name == "Product A"));
    assert!(order.items.iter().any(|i| i.product_name == "Product B"));
}

#[tokio::test]
async fn end_to_end_multiple_orders_queries_work() {
    let storage = Arc::new(InMemoryStorage::new());
    let store = OpossumStore::new(Arc::clone(&storage), FakeClock::new(1000));
    let projection_store =
        StorageBackendProjectionStore::new(Arc::clone(&storage), "E2EOrders".to_string());
    let runner = ProjectionRunner::new(Arc::clone(&storage), E2EOrderProjection, projection_store);

    let order1_id = "order-1";
    let order2_id = "order-2";
    let order3_id = "order-3";

    let mut events = Vec::new();

    // Order 1
    events.push(EventData {
        event_id: uuid::Uuid::new_v4().to_string(),
        event: DomainEvent {
            event_type: "E2EOrderCreatedEvent".to_string(),
            data: serde_json::to_string(&serde_json::json!({
                "order_id": order1_id,
                "customer_name": "Customer 1",
                "customer_email": "c1@test.com"
            }))
            .unwrap(),
            tags: vec![Tag {
                key: "orderId".to_string(),
                value: order1_id.to_string(),
            }],
        },
        metadata: None,
    });
    events.push(EventData {
        event_id: uuid::Uuid::new_v4().to_string(),
        event: DomainEvent {
            event_type: "E2EItemAddedEvent".to_string(),
            data: serde_json::to_string(&serde_json::json!({
                "product_name": "Product A",
                "price": 100.0
            }))
            .unwrap(),
            tags: vec![Tag {
                key: "orderId".to_string(),
                value: order1_id.to_string(),
            }],
        },
        metadata: None,
    });

    // Order 2
    events.push(EventData {
        event_id: uuid::Uuid::new_v4().to_string(),
        event: DomainEvent {
            event_type: "E2EOrderCreatedEvent".to_string(),
            data: serde_json::to_string(&serde_json::json!({
                "order_id": order2_id,
                "customer_name": "Customer 2",
                "customer_email": "c2@test.com"
            }))
            .unwrap(),
            tags: vec![Tag {
                key: "orderId".to_string(),
                value: order2_id.to_string(),
            }],
        },
        metadata: None,
    });
    events.push(EventData {
        event_id: uuid::Uuid::new_v4().to_string(),
        event: DomainEvent {
            event_type: "E2EItemAddedEvent".to_string(),
            data: serde_json::to_string(&serde_json::json!({
                "product_name": "Product B",
                "price": 200.0
            }))
            .unwrap(),
            tags: vec![Tag {
                key: "orderId".to_string(),
                value: order2_id.to_string(),
            }],
        },
        metadata: None,
    });
    events.push(EventData {
        event_id: uuid::Uuid::new_v4().to_string(),
        event: DomainEvent {
            event_type: "E2EItemAddedEvent".to_string(),
            data: serde_json::to_string(&serde_json::json!({
                "product_name": "Product C",
                "price": 300.0
            }))
            .unwrap(),
            tags: vec![Tag {
                key: "orderId".to_string(),
                value: order2_id.to_string(),
            }],
        },
        metadata: None,
    });

    // Order 3
    events.push(EventData {
        event_id: uuid::Uuid::new_v4().to_string(),
        event: DomainEvent {
            event_type: "E2EOrderCreatedEvent".to_string(),
            data: serde_json::to_string(&serde_json::json!({
                "order_id": order3_id,
                "customer_name": "Customer 3",
                "customer_email": "c3@test.com"
            }))
            .unwrap(),
            tags: vec![Tag {
                key: "orderId".to_string(),
                value: order3_id.to_string(),
            }],
        },
        metadata: None,
    });
    events.push(EventData {
        event_id: uuid::Uuid::new_v4().to_string(),
        event: DomainEvent {
            event_type: "E2EItemAddedEvent".to_string(),
            data: serde_json::to_string(&serde_json::json!({
                "product_name": "Product D",
                "price": 50.0
            }))
            .unwrap(),
            tags: vec![Tag {
                key: "orderId".to_string(),
                value: order3_id.to_string(),
            }],
        },
        metadata: None,
    });

    store.append_async(events, None).await.unwrap();

    let all_events = store
        .read_async(Query::all(), None, None, None)
        .await
        .unwrap();
    runner.process_events(&all_events).await.unwrap();

    let p_store = StorageBackendProjectionStore::new(Arc::clone(&storage), "E2EOrders".to_string());
    let all_orders: Vec<E2EOrderState> = p_store.get_all().await.unwrap();

    assert_eq!(all_orders.len(), 3);

    let expensive_orders: Vec<_> = all_orders
        .into_iter()
        .filter(|o| o.total_amount >= 200.0)
        .collect();
    assert_eq!(expensive_orders.len(), 1);
    assert_eq!(expensive_orders[0].order_id, order2_id);
    assert!(!expensive_orders.iter().any(|o| o.total_amount == 100.0));
}

#[tokio::test]
async fn end_to_end_incremental_update_updates_projection() {
    let storage = Arc::new(InMemoryStorage::new());
    let store = OpossumStore::new(Arc::clone(&storage), FakeClock::new(1000));
    let projection_store =
        StorageBackendProjectionStore::new(Arc::clone(&storage), "E2EOrders".to_string());
    let runner = ProjectionRunner::new(Arc::clone(&storage), E2EOrderProjection, projection_store);

    let order_id = "order-inc";

    let initial_events = vec![
        EventData {
            event_id: uuid::Uuid::new_v4().to_string(),
            event: DomainEvent {
                event_type: "E2EOrderCreatedEvent".to_string(),
                data: serde_json::to_string(&serde_json::json!({
                    "order_id": order_id,
                    "customer_name": "Customer",
                    "customer_email": "customer@test.com"
                }))
                .unwrap(),
                tags: vec![Tag {
                    key: "orderId".to_string(),
                    value: order_id.to_string(),
                }],
            },
            metadata: None,
        },
        EventData {
            event_id: uuid::Uuid::new_v4().to_string(),
            event: DomainEvent {
                event_type: "E2EItemAddedEvent".to_string(),
                data: serde_json::to_string(&serde_json::json!({
                    "product_name": "Product A",
                    "price": 100.0
                }))
                .unwrap(),
                tags: vec![Tag {
                    key: "orderId".to_string(),
                    value: order_id.to_string(),
                }],
            },
            metadata: None,
        },
    ];

    store.append_async(initial_events, None).await.unwrap();

    let all_events = store
        .read_async(Query::all(), None, None, None)
        .await
        .unwrap();
    runner.process_events(&all_events).await.unwrap();

    let p_store = StorageBackendProjectionStore::new(Arc::clone(&storage), "E2EOrders".to_string());
    let order_before: Option<E2EOrderState> = p_store.get(order_id).await.unwrap();
    assert_eq!(order_before.as_ref().unwrap().item_count, 1);
    assert_eq!(order_before.as_ref().unwrap().total_amount, 100.0);

    let new_events = vec![EventData {
        event_id: uuid::Uuid::new_v4().to_string(),
        event: DomainEvent {
            event_type: "E2EItemAddedEvent".to_string(),
            data: serde_json::to_string(&serde_json::json!({
                "product_name": "Product B",
                "price": 200.0
            }))
            .unwrap(),
            tags: vec![Tag {
                key: "orderId".to_string(),
                value: order_id.to_string(),
            }],
        },
        metadata: None,
    }];

    store.append_async(new_events, None).await.unwrap();

    let checkpoint = runner.get_checkpoint().await.unwrap();
    let start_pos = checkpoint.map(|c| c.last_position);
    let incremental_events = store
        .read_async(Query::all(), start_pos, None, None)
        .await
        .unwrap();

    runner.process_events(&incremental_events).await.unwrap();

    let order_after: Option<E2EOrderState> = p_store.get(order_id).await.unwrap();
    assert_eq!(order_after.as_ref().unwrap().item_count, 2);
    assert_eq!(order_after.as_ref().unwrap().total_amount, 300.0);
}

#[tokio::test]
async fn end_to_end_order_cancellation_removes_projection() {
    let storage = Arc::new(InMemoryStorage::new());
    let store = OpossumStore::new(Arc::clone(&storage), FakeClock::new(1000));
    let projection_store =
        StorageBackendProjectionStore::new(Arc::clone(&storage), "E2EOrders".to_string());
    let runner = ProjectionRunner::new(Arc::clone(&storage), E2EOrderProjection, projection_store);

    let order_id = "order-cancel";

    let events = vec![
        EventData {
            event_id: uuid::Uuid::new_v4().to_string(),
            event: DomainEvent {
                event_type: "E2EOrderCreatedEvent".to_string(),
                data: serde_json::to_string(&serde_json::json!({
                    "order_id": order_id,
                    "customer_name": "Customer",
                    "customer_email": "customer@test.com"
                }))
                .unwrap(),
                tags: vec![Tag {
                    key: "orderId".to_string(),
                    value: order_id.to_string(),
                }],
            },
            metadata: None,
        },
        EventData {
            event_id: uuid::Uuid::new_v4().to_string(),
            event: DomainEvent {
                event_type: "E2EItemAddedEvent".to_string(),
                data: serde_json::to_string(&serde_json::json!({
                    "product_name": "Product A",
                    "price": 100.0
                }))
                .unwrap(),
                tags: vec![Tag {
                    key: "orderId".to_string(),
                    value: order_id.to_string(),
                }],
            },
            metadata: None,
        },
        EventData {
            event_id: uuid::Uuid::new_v4().to_string(),
            event: DomainEvent {
                event_type: "E2EOrderCancelledEvent".to_string(),
                data: "{}".to_string(),
                tags: vec![Tag {
                    key: "orderId".to_string(),
                    value: order_id.to_string(),
                }],
            },
            metadata: None,
        },
    ];

    store.append_async(events, None).await.unwrap();

    let all_events = store
        .read_async(Query::all(), None, None, None)
        .await
        .unwrap();
    runner.process_events(&all_events).await.unwrap();

    let p_store = StorageBackendProjectionStore::new(Arc::clone(&storage), "E2EOrders".to_string());
    let order_after: Option<E2EOrderState> = p_store.get(order_id).await.unwrap();
    assert!(order_after.is_none());
}

#[tokio::test]
async fn end_to_end_checkpoint_management_tracks_progress() {
    let storage = Arc::new(InMemoryStorage::new());
    let store = OpossumStore::new(Arc::clone(&storage), FakeClock::new(1000));
    let projection_store =
        StorageBackendProjectionStore::new(Arc::clone(&storage), "E2EOrders".to_string());
    let runner = ProjectionRunner::new(Arc::clone(&storage), E2EOrderProjection, projection_store);

    let initial_checkpoint = runner.get_checkpoint().await.unwrap();
    assert!(initial_checkpoint.is_none());

    let order_id = "order-checkpoint";

    let events = vec![
        EventData {
            event_id: uuid::Uuid::new_v4().to_string(),
            event: DomainEvent {
                event_type: "E2EOrderCreatedEvent".to_string(),
                data: serde_json::to_string(&serde_json::json!({
                    "order_id": order_id,
                    "customer_name": "Customer",
                    "customer_email": "customer@test.com"
                }))
                .unwrap(),
                tags: vec![Tag {
                    key: "orderId".to_string(),
                    value: order_id.to_string(),
                }],
            },
            metadata: None,
        },
        EventData {
            event_id: uuid::Uuid::new_v4().to_string(),
            event: DomainEvent {
                event_type: "E2EItemAddedEvent".to_string(),
                data: serde_json::to_string(&serde_json::json!({
                    "product_name": "Product A",
                    "price": 100.0
                }))
                .unwrap(),
                tags: vec![Tag {
                    key: "orderId".to_string(),
                    value: order_id.to_string(),
                }],
            },
            metadata: None,
        },
    ];

    store.append_async(events, None).await.unwrap();

    let all_events = store
        .read_async(Query::all(), None, None, None)
        .await
        .unwrap();
    runner.process_events(&all_events).await.unwrap();

    let checkpoint_after = runner.get_checkpoint().await.unwrap();
    assert!(checkpoint_after.is_some());
    assert!(checkpoint_after.unwrap().last_position > 0);
}
