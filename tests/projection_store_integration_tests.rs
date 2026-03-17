mod common;

use common::InMemoryStorage;
use marmosa::projections::{ProjectionStore, StorageBackendProjectionStore};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Default)]
pub struct TestOrderState {
    pub order_id: String,
    pub customer_name: String,
    pub total_amount: i32,
}

async fn create_store() -> StorageBackendProjectionStore<Arc<InMemoryStorage>, TestOrderState> {
    let _clock = common::FakeClock::new(0);
    let storage = Arc::new(InMemoryStorage::new());
    let store_name = format!("TestOrders_{}", Uuid::new_v4().simple());
    StorageBackendProjectionStore::new(storage, store_name)
}

#[tokio::test]
async fn save_with_valid_state_saves_file() {
    let store = create_store().await;
    let order_id = Uuid::new_v4().to_string();
    let state = TestOrderState {
        order_id: order_id.clone(),
        customer_name: "Customer A".to_string(),
        total_amount: 100,
    };

    store.save(&order_id, &state).await.unwrap();

    let retrieved = store.get(&order_id).await.unwrap();
    assert!(retrieved.is_some());
    let retrieved = retrieved.unwrap();
    assert_eq!(state.order_id, retrieved.order_id);
    assert_eq!(state.customer_name, retrieved.customer_name);
    assert_eq!(state.total_amount, retrieved.total_amount);
}

#[tokio::test]
async fn get_with_non_existent_key_returns_null() {
    let store = create_store().await;
    let non_existent_id = Uuid::new_v4().to_string();

    let result = store.get(&non_existent_id).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn save_with_existing_key_updates_file() {
    let store = create_store().await;
    let order_id = Uuid::new_v4().to_string();
    let initial_state = TestOrderState {
        order_id: order_id.clone(),
        customer_name: "Customer A".to_string(),
        total_amount: 100,
    };
    let updated_state = TestOrderState {
        order_id: order_id.clone(),
        customer_name: "Customer A".to_string(),
        total_amount: 200,
    };

    store.save(&order_id, &initial_state).await.unwrap();
    store.save(&order_id, &updated_state).await.unwrap();

    let retrieved = store.get(&order_id).await.unwrap();
    assert!(retrieved.is_some());
    assert_eq!(200, retrieved.unwrap().total_amount);
}

#[tokio::test]
async fn delete_with_existing_key_removes_file() {
    let store = create_store().await;
    let order_id = Uuid::new_v4().to_string();
    let state = TestOrderState {
        order_id: order_id.clone(),
        customer_name: "Customer A".to_string(),
        total_amount: 100,
    };

    store.save(&order_id, &state).await.unwrap();
    store.delete(&order_id).await.unwrap();

    let retrieved = store.get(&order_id).await.unwrap();
    assert!(retrieved.is_none());
}

#[tokio::test]
async fn delete_with_non_existent_key_does_not_throw() {
    let store = create_store().await;
    let order_id = Uuid::new_v4().to_string();

    let result = store.delete(&order_id).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn get_all_with_multiple_states_returns_all() {
    let store = create_store().await;
    let order_id_1 = Uuid::new_v4().to_string();
    let state_1 = TestOrderState {
        order_id: order_id_1.clone(),
        customer_name: "Customer A".to_string(),
        total_amount: 100,
    };

    let order_id_2 = Uuid::new_v4().to_string();
    let state_2 = TestOrderState {
        order_id: order_id_2.clone(),
        customer_name: "Customer B".to_string(),
        total_amount: 150,
    };

    store.save(&order_id_1, &state_1).await.unwrap();
    store.save(&order_id_2, &state_2).await.unwrap();

    let all_states = store.get_all().await.unwrap();
    assert_eq!(2, all_states.len());
    assert!(all_states.iter().any(|s| s.order_id == order_id_1));
    assert!(all_states.iter().any(|s| s.order_id == order_id_2));
}

#[tokio::test]
async fn get_all_with_no_states_returns_empty_list() {
    let store = create_store().await;
    let all_states = store.get_all().await.unwrap();
    assert_eq!(0, all_states.len());
}

#[tokio::test]
async fn save_with_special_characters_in_key_handles_safely() {
    let store = create_store().await;
    let order_id = "order/with:special*chars?".to_string();
    let state = TestOrderState {
        order_id: order_id.clone(),
        customer_name: "Customer Spec".to_string(),
        total_amount: 99,
    };

    let result = store.save(&order_id, &state).await;
    assert!(result.is_ok());

    let retrieved = store.get(&order_id).await.unwrap();
    assert!(retrieved.is_some());
    assert_eq!(state.order_id, retrieved.unwrap().order_id);
}
