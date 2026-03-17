mod common;

use common::{FakeClock, InMemoryStorage};
use marmosa::domain::{DomainEvent, EventData, EventRecord, Query, QueryItem, Tag};
use marmosa::event_store::{EventStore, OpossumStore};
use marmosa::projections::{
    ProjectionDefinition, ProjectionRunner, ProjectionStore, StorageBackendProjectionStore,
};
use serde::{Deserialize, Serialize};
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

#[derive(Serialize, Deserialize, Debug, Clone)]
struct AccountClosed {
    account_id: String,
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
                    "AccountClosedEvent".to_string(),
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
            "AccountClosedEvent" => {
                None // Deletes
            }
            _ => state,
        }
    }
}

#[tokio::test]
async fn rebuild_projection_after_events_added_builds_correct_state() {
    let storage = Arc::new(InMemoryStorage::new());
    let store = OpossumStore::new(Arc::clone(&storage), FakeClock::new(1000));

    let account_id = "acc123".to_string();

    let events = vec![
        create_event(
            "AccountCreatedEvent",
            &account_id,
            &AccountCreated {
                account_id: account_id.clone(),
                owner_name: "John Doe".to_string(),
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

    let projection_store =
        StorageBackendProjectionStore::new(Arc::clone(&storage), "AccountBalance".to_string());

    let runner = ProjectionRunner::new(
        Arc::clone(&storage),
        AccountBalanceProjection,
        projection_store,
    );

    // Act - Rebuild
    runner.rebuild(&store).await.unwrap();

    // Assert
    let p_store =
        StorageBackendProjectionStore::new(Arc::clone(&storage), "AccountBalance".to_string());
    let state: AccountBalanceState = p_store.get(&account_id).await.unwrap().unwrap();
    assert_eq!(state.balance, 1300.0);
    assert_eq!(state.transaction_count, 3);
}

#[tokio::test]
async fn rebuild_projection_with_existing_state_replaces_state() {
    let storage = Arc::new(InMemoryStorage::new());
    let store = OpossumStore::new(Arc::clone(&storage), FakeClock::new(1000));
    let account_id = "acc456".to_string();

    let initial_events = vec![create_event(
        "AccountCreatedEvent",
        &account_id,
        &AccountCreated {
            account_id: account_id.clone(),
            owner_name: "Jane Doe".to_string(),
            initial_balance: 100.0,
        },
    )];

    store.append_async(initial_events, None).await.unwrap();

    let projection_store =
        StorageBackendProjectionStore::new(Arc::clone(&storage), "AccountBalance".to_string());

    let runner = ProjectionRunner::new(
        Arc::clone(&storage),
        AccountBalanceProjection,
        projection_store,
    );

    let all_events = store
        .read_async(Query::all(), None, None, None)
        .await
        .unwrap();
    runner.process_events(&all_events).await.unwrap(); // first build via sync

    let p_store =
        StorageBackendProjectionStore::new(Arc::clone(&storage), "AccountBalance".to_string());
    let state: AccountBalanceState = p_store.get(&account_id).await.unwrap().unwrap();
    assert_eq!(state.balance, 100.0);

    let new_events = vec![create_event(
        "MoneyDepositedEvent",
        &account_id,
        &MoneyDeposited {
            account_id: account_id.clone(),
            amount: 900.0,
        },
    )];

    store.append_async(new_events, None).await.unwrap();

    // Act - Rebuild again! (Should wipe old and re-process from 0)
    runner.rebuild(&store).await.unwrap();

    let p_store =
        StorageBackendProjectionStore::new(Arc::clone(&storage), "AccountBalance".to_string());
    let state: AccountBalanceState = p_store.get(&account_id).await.unwrap().unwrap();
    assert_eq!(state.balance, 1000.0);
    assert_eq!(state.transaction_count, 2);
}

#[tokio::test]
async fn rebuild_projection_with_deletion_removes_projection() {
    let storage = Arc::new(InMemoryStorage::new());
    let store = OpossumStore::new(Arc::clone(&storage), FakeClock::new(1000));
    let account_id = "acc789".to_string();

    let events = vec![
        create_event(
            "AccountCreatedEvent",
            &account_id,
            &AccountCreated {
                account_id: account_id.clone(),
                owner_name: "Delete Me".to_string(),
                initial_balance: 1000.0,
            },
        ),
        create_event(
            "AccountClosedEvent",
            &account_id,
            &AccountClosed {
                account_id: account_id.clone(),
            },
        ),
    ];

    store.append_async(events, None).await.unwrap();

    let projection_store =
        StorageBackendProjectionStore::new(Arc::clone(&storage), "AccountBalance".to_string());

    let runner = ProjectionRunner::new(
        Arc::clone(&storage),
        AccountBalanceProjection,
        projection_store,
    );

    runner.rebuild(&store).await.unwrap();

    let p_store =
        StorageBackendProjectionStore::new(Arc::clone(&storage), "AccountBalance".to_string());
    let state: Option<AccountBalanceState> = p_store.get(&account_id).await.unwrap();

    assert!(state.is_none());
}
