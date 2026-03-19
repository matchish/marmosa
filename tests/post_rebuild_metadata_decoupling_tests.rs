mod common;

use common::{FakeClock, InMemoryStorage};
use marmosa::domain::{DomainEvent, EventData, EventRecord, Query, QueryItem, Tag};
use marmosa::event_store::{EventStore, MarmosaStore};
use marmosa::ports::StorageBackend;
use marmosa::projections::{
    ProjectionDefinition, ProjectionMetadata, ProjectionRunner, ProjectionStore, StorageBackendProjectionStore,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

// --- DTOs ---

#[derive(Serialize, Deserialize, Debug, Clone)]
struct AccountCreated {
    account_id: String,
    owner_name: String,
    initial_balance: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct MoneyDeposited {
    account_id: String,
    amount: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct MoneyWithdrawn {
    account_id: String,
    amount: f64,
}

fn create_event(event_type: &str, account_id: &str, data: &impl Serialize) -> EventData {
    let data_str = serde_json::to_string(data).unwrap();
    EventData {
        event_id: uuid::Uuid::new_v4().to_string(),
        event: DomainEvent {
            event_type: event_type.to_string(),
            data: data_str,
            tags: vec![Tag {
                key: "accountId".to_string(),
                value: account_id.to_string(),
            }],
        },
        metadata: None,
    }
}

// --- Projection State ---

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct AccountBalanceState {
    pub account_id: String,
    pub owner_name: String,
    pub balance: f64,
    pub transaction_count: u32,
}

// --- Projection Definition ---

pub struct AccountBalanceProjection;

impl ProjectionDefinition for AccountBalanceProjection {
    type State = AccountBalanceState;

    fn projection_name(&self) -> &str {
        "AccountBalance"
    }

    fn event_types(&self) -> Query {
        Query {
            items: vec![QueryItem {
                event_types: vec![
                    "AccountCreatedEvent".to_string(),
                    "MoneyDepositedEvent".to_string(),
                    "MoneyWithdrawnEvent".to_string(),
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
            .find(|t| t.key == "accountId")
            .map(|t| t.value.clone())
    }

    fn apply(&self, state: Option<Self::State>, event: &EventRecord) -> Option<Self::State> {
        match event.event.event_type.as_str() {
            "AccountCreatedEvent" => {
                if let Ok(data) = serde_json::from_str::<AccountCreated>(
                    event.event.data.replace("\\\"", "\"").trim_matches('"'),
                ) {
                    Some(AccountBalanceState {
                        account_id: data.account_id,
                        owner_name: data.owner_name,
                        balance: data.initial_balance,
                        transaction_count: 1,
                    })
                } else if let Ok(data) = serde_json::from_str::<AccountCreated>(&event.event.data) {
                    Some(AccountBalanceState {
                        account_id: data.account_id,
                        owner_name: data.owner_name,
                        balance: data.initial_balance,
                        transaction_count: 1,
                    })
                } else {
                    state
                }
            }
            "MoneyDepositedEvent" => {
                if let Some(mut s) = state {
                    if let Ok(data) = serde_json::from_str::<MoneyDeposited>(
                        event.event.data.replace("\\\"", "\"").trim_matches('"'),
                    ) {
                        s.balance += data.amount;
                        s.transaction_count += 1;
                        Some(s)
                    } else if let Ok(data) =
                        serde_json::from_str::<MoneyDeposited>(&event.event.data)
                    {
                        s.balance += data.amount;
                        s.transaction_count += 1;
                        Some(s)
                    } else {
                        Some(s)
                    }
                } else {
                    None
                }
            }
            "MoneyWithdrawnEvent" => {
                if let Some(mut s) = state {
                    if let Ok(data) = serde_json::from_str::<MoneyWithdrawn>(
                        event.event.data.replace("\\\"", "\"").trim_matches('"'),
                    ) {
                        s.balance -= data.amount;
                        s.transaction_count += 1;
                        Some(s)
                    } else if let Ok(data) =
                        serde_json::from_str::<MoneyWithdrawn>(&event.event.data)
                    {
                        s.balance -= data.amount;
                        s.transaction_count += 1;
                        Some(s)
                    } else {
                        Some(s)
                    }
                } else {
                    None
                }
            }
            _ => state,
        }
    }
}

async fn setup() -> (
    Arc<InMemoryStorage>,
    MarmosaStore<Arc<InMemoryStorage>, FakeClock>,
    ProjectionRunner<Arc<InMemoryStorage>, AccountBalanceState, AccountBalanceProjection, StorageBackendProjectionStore<Arc<InMemoryStorage>, AccountBalanceState, marmosa::projections::NoopProjectionTagProvider, marmosa::projections::NoopClock>>,
) {
    let storage = Arc::new(InMemoryStorage::new());
    let store = MarmosaStore::new(Arc::clone(&storage), FakeClock::new(1000));
    let projection_store = StorageBackendProjectionStore::new(Arc::clone(&storage), "AccountBalance".to_string());
    let runner = ProjectionRunner::new(Arc::clone(&storage), AccountBalanceProjection, projection_store);

    (storage, store, runner)
}

#[tokio::test]
async fn post_rebuild_metadata_index_file_exists() {
    let (storage, store, runner) = setup().await;
    let account_id = uuid::Uuid::new_v4().to_string();

    let events = vec![
        create_event(
            "AccountCreatedEvent",
            &account_id,
            &AccountCreated {
                account_id: account_id.clone(),
                owner_name: "Alice".to_string(),
                initial_balance: 500.0,
            },
        ),
        create_event(
            "MoneyDepositedEvent",
            &account_id,
            &MoneyDeposited {
                account_id: account_id.clone(),
                amount: 200.0,
            },
        ),
    ];

    store.append_async(events, None).await.unwrap();

    runner.rebuild(&store).await.unwrap();

    let metadata_index_file = "Projections/AccountBalance/Metadata/index.json";
    let exists = storage.read_file(metadata_index_file).await.is_ok();
    assert!(exists, "Metadata/index.json should exist after rebuild in rust implementation");
}

#[tokio::test]
async fn post_rebuild_get_returns_correct_state() {
    let (storage, store, runner) = setup().await;
    let account_id = uuid::Uuid::new_v4().to_string();

    let events = vec![
        create_event(
            "AccountCreatedEvent",
            &account_id,
            &AccountCreated {
                account_id: account_id.clone(),
                owner_name: "Bob".to_string(),
                initial_balance: 1000.0,
            },
        ),
        create_event(
            "MoneyDepositedEvent",
            &account_id,
            &MoneyDeposited {
                account_id: account_id.clone(),
                amount: 500.0,
            },
        ),
        create_event(
            "MoneyWithdrawnEvent",
            &account_id,
            &MoneyWithdrawn {
                account_id: account_id.clone(),
                amount: 200.0,
            },
        ),
    ];

    store.append_async(events, None).await.unwrap();
    runner.rebuild(&store).await.unwrap();

    let projection_store = StorageBackendProjectionStore::new(Arc::clone(&storage), "AccountBalance".to_string());
    let state: Option<AccountBalanceState> = projection_store.get(&account_id).await.unwrap();
    
    assert!(state.is_some());
    let state = state.unwrap();
    assert_eq!(state.account_id, account_id);
    assert_eq!(state.owner_name, "Bob");
    assert_eq!(state.balance, 1300.0);
    assert_eq!(state.transaction_count, 3);
}

#[tokio::test]
async fn post_rebuild_get_all_returns_all_projections() {
    let (storage, store, runner) = setup().await;

    let mut ids = vec![];
    let mut events = vec![];
    for i in 0..5 {
        let id = uuid::Uuid::new_v4().to_string();
        ids.push(id.clone());
        events.push(create_event(
            "AccountCreatedEvent",
            &id,
            &AccountCreated {
                account_id: id.clone(),
                owner_name: format!("User{}", i),
                initial_balance: 100.0 * (i as f64 + 1.0),
            },
        ));
    }

    store.append_async(events, None).await.unwrap();
    runner.rebuild(&store).await.unwrap();

    let projection_store = StorageBackendProjectionStore::new(Arc::clone(&storage), "AccountBalance".to_string());
    let all: Vec<AccountBalanceState> = projection_store.get_all().await.unwrap();

    assert_eq!(all.len(), 5);
    for id in &ids {
        assert!(all.iter().any(|s| s.account_id == *id));
    }
}

#[tokio::test]
async fn post_rebuild_query_filters_correctly() {
    let (storage, store, runner) = setup().await;
    let id1 = uuid::Uuid::new_v4().to_string();
    let id2 = uuid::Uuid::new_v4().to_string();

    let events = vec![
        create_event(
            "AccountCreatedEvent",
            &id1,
            &AccountCreated {
                account_id: id1.clone(),
                owner_name: "Charlie".to_string(),
                initial_balance: 100.0,
            },
        ),
        create_event(
            "AccountCreatedEvent",
            &id2,
            &AccountCreated {
                account_id: id2.clone(),
                owner_name: "Diana".to_string(),
                initial_balance: 200.0,
            },
        ),
    ];

    store.append_async(events, None).await.unwrap();
    runner.rebuild(&store).await.unwrap();

    let projection_store = StorageBackendProjectionStore::new(Arc::clone(&storage), "AccountBalance".to_string());
    let all: Vec<AccountBalanceState> = projection_store.get_all().await.unwrap();
    let results: Vec<AccountBalanceState> = all.into_iter().filter(|s| s.balance >= 200.0).collect();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].owner_name, "Diana");
}

#[tokio::test]
async fn post_rebuild_normal_save_starts_metadata_version_from_two() {
    let (storage, store, runner) = setup().await;
    let account_id = uuid::Uuid::new_v4().to_string();

    let events = vec![create_event(
        "AccountCreatedEvent",
        &account_id,
        &AccountCreated {
            account_id: account_id.clone(),
            owner_name: "Eve".to_string(),
            initial_balance: 100.0,
        },
    )];

    store.append_async(events, None).await.unwrap();
    runner.rebuild(&store).await.unwrap();

    let deposit_event = create_event(
        "MoneyDepositedEvent",
        &account_id,
        &MoneyDeposited {
            account_id: account_id.clone(),
            amount: 50.0,
        },
    );
    store.append_async(vec![deposit_event], None).await.unwrap();

    let all_events = store.read_async(Query::all(), None, None, None).await.unwrap();

    runner.process_events(&all_events[1..]).await.unwrap();

    let projection_store = StorageBackendProjectionStore::new(Arc::clone(&storage), "AccountBalance".to_string());
    let state: Option<AccountBalanceState> = projection_store.get(&account_id).await.unwrap();

    assert!(state.is_some());
    assert_eq!(state.unwrap().balance, 150.0);

    let metadata_index_file = "Projections/AccountBalance/Metadata/index.json";
    let exists = storage.read_file(metadata_index_file).await.is_ok();
    assert!(exists, "Metadata/index.json should be re-created by normal SaveAsync after rebuild.");

    let index_json = storage.read_file(metadata_index_file).await.unwrap();
    let index: BTreeMap<String, ProjectionMetadata> = serde_json::from_slice(&index_json).unwrap();
    
    assert!(index.contains_key(&account_id));
    assert_eq!(index.get(&account_id).unwrap().version, 2);
}
