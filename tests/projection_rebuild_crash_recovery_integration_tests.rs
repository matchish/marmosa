mod common;

use common::{FakeClock, InMemoryStorage};
use marmosa::domain::{DomainEvent, EventData, EventRecord, Query, QueryItem, Tag};
use marmosa::event_store::{EventStore, OpossumStore};
use marmosa::ports::StorageBackend;
use marmosa::projections::{
    ProjectionDefinition, ProjectionRunner, ProjectionStore, ProjectionTagProvider,
    StorageBackendProjectionStore,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CrashRecoveryCreatedEvent {
    account_id: String,
    owner_name: String,
    initial_balance: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CrashRecoveryDepositEvent {
    account_id: String,
    amount: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct CrashRecoveryState {
    account_id: String,
    owner_name: String,
    balance: f64,
    transaction_count: u32,
}

struct CrashRecoveryProjection;

impl ProjectionDefinition for CrashRecoveryProjection {
    type State = CrashRecoveryState;

    fn projection_name(&self) -> &str {
        "CrashRecoveryBalance"
    }

    fn event_types(&self) -> Query {
        Query {
            items: vec![QueryItem {
                event_types: vec![
                    "CrashRecoveryCreatedEvent".to_string(),
                    "CrashRecoveryDepositEvent".to_string(),
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
            .find(|tag| tag.key == "accountId")
            .map(|tag| tag.value.clone())
    }

    fn apply(&self, state: Option<Self::State>, event: &EventRecord) -> Option<Self::State> {
        match event.event.event_type.as_str() {
            "CrashRecoveryCreatedEvent" => {
                let created = serde_json::from_str::<CrashRecoveryCreatedEvent>(&event.event.data)
                    .unwrap();
                Some(CrashRecoveryState {
                    account_id: created.account_id,
                    owner_name: created.owner_name,
                    balance: created.initial_balance,
                    transaction_count: 1,
                })
            }
            "CrashRecoveryDepositEvent" => {
                if let Some(mut current) = state {
                    let deposited =
                        serde_json::from_str::<CrashRecoveryDepositEvent>(&event.event.data)
                            .unwrap();
                    current.balance += deposited.amount;
                    current.transaction_count += 1;
                    Some(current)
                } else {
                    None
                }
            }
            _ => state,
        }
    }
}

struct CrashRecoveryTagProvider;

impl ProjectionTagProvider<CrashRecoveryState> for CrashRecoveryTagProvider {
    fn get_tags(&self, _state: &CrashRecoveryState) -> Vec<Tag> {
        vec![Tag {
            key: "status".to_string(),
            value: "active".to_string(),
        }]
    }
}

fn created_event(account_id: &str, owner_name: &str, initial_balance: f64) -> EventData {
    EventData {
        event_id: uuid::Uuid::new_v4().to_string(),
        event: DomainEvent {
            event_type: "CrashRecoveryCreatedEvent".to_string(),
            data: serde_json::to_string(&CrashRecoveryCreatedEvent {
                account_id: account_id.to_string(),
                owner_name: owner_name.to_string(),
                initial_balance,
            })
            .unwrap(),
            tags: vec![Tag {
                key: "accountId".to_string(),
                value: account_id.to_string(),
            }],
        },
        metadata: None,
    }
}

fn deposit_event(account_id: &str, amount: f64) -> EventData {
    EventData {
        event_id: uuid::Uuid::new_v4().to_string(),
        event: DomainEvent {
            event_type: "CrashRecoveryDepositEvent".to_string(),
            data: serde_json::to_string(&CrashRecoveryDepositEvent {
                account_id: account_id.to_string(),
                amount,
            })
            .unwrap(),
            tags: vec![Tag {
                key: "accountId".to_string(),
                value: account_id.to_string(),
            }],
        },
        metadata: None,
    }
}

#[tokio::test]
async fn rebuild_projection_after_interrupted_sequence_recovers_final_balance() {
    let storage = Arc::new(InMemoryStorage::new());
    let store = OpossumStore::new(Arc::clone(&storage), FakeClock::new(0));
    let account_id = uuid::Uuid::new_v4().to_string();

    store
        .append_async(
            vec![
                created_event(&account_id, "Alice", 1000.0),
                deposit_event(&account_id, 200.0),
            ],
            None,
        )
        .await
        .unwrap();

    let orphan_position = 2_u64;
    let orphan_record = EventRecord {
        position: orphan_position,
        event_id: uuid::Uuid::new_v4().to_string(),
        event: DomainEvent {
            event_type: "CrashRecoveryDepositEvent".to_string(),
            data: serde_json::to_string(&CrashRecoveryDepositEvent {
                account_id: account_id.clone(),
                amount: 300.0,
            })
            .unwrap(),
            tags: vec![Tag {
                key: "accountId".to_string(),
                value: account_id.clone(),
            }],
        },
        metadata: None,
        timestamp: 0,
    };
    let orphan_file = format!("Events/{orphan_position:010}.json");
    storage
        .write_file(&orphan_file, &serde_json::to_vec(&orphan_record).unwrap())
        .await
        .unwrap();

    let projection_store =
        StorageBackendProjectionStore::new(Arc::clone(&storage), "CrashRecoveryBalance".to_string());
    let runner = ProjectionRunner::new(
        Arc::clone(&storage),
        CrashRecoveryProjection,
        projection_store,
    );

    runner.rebuild(&store).await.unwrap();

    let read_store: StorageBackendProjectionStore<_, CrashRecoveryState> =
        StorageBackendProjectionStore::new(Arc::clone(&storage), "CrashRecoveryBalance".to_string());
    let state = read_store.get(&account_id).await.unwrap().unwrap();
    assert_eq!(state.balance, 1500.0);
    assert_eq!(state.transaction_count, 3);
}

#[tokio::test]
async fn rebuild_projection_saves_checkpoint_and_no_rebuild_journal_artifacts() {
    let storage = Arc::new(InMemoryStorage::new());
    let store = OpossumStore::new(Arc::clone(&storage), FakeClock::new(0));
    let account_id = uuid::Uuid::new_v4().to_string();

    store
        .append_async(vec![created_event(&account_id, "Bob", 500.0)], None)
        .await
        .unwrap();

    let projection_store =
        StorageBackendProjectionStore::new(Arc::clone(&storage), "CrashRecoveryBalance".to_string());
    let runner = ProjectionRunner::new(
        Arc::clone(&storage),
        CrashRecoveryProjection,
        projection_store,
    );

    runner.rebuild(&store).await.unwrap();

    let checkpoint = runner.get_checkpoint().await.unwrap();
    assert!(checkpoint.is_some());
    assert_eq!(checkpoint.unwrap().last_position, 0);

    let checkpoint_files = storage.read_dir("Projections/_checkpoints").await.unwrap();
    assert!(
        checkpoint_files
            .iter()
            .all(|path| !path.ends_with(".rebuild.json") && !path.ends_with(".rebuild.tags.json"))
    );
}

#[tokio::test]
async fn rebuild_with_tag_provider_restores_index_consistently() {
    let storage = Arc::new(InMemoryStorage::new());
    let store = OpossumStore::new(Arc::clone(&storage), FakeClock::new(0));
    let account_id_1 = uuid::Uuid::new_v4().to_string();
    let account_id_2 = uuid::Uuid::new_v4().to_string();

    store
        .append_async(
            vec![
                created_event(&account_id_1, "Alice", 1000.0),
                created_event(&account_id_2, "Bob", 500.0),
                deposit_event(&account_id_1, 250.0),
            ],
            None,
        )
        .await
        .unwrap();

    let projection_store: StorageBackendProjectionStore<
        _,
        CrashRecoveryState,
        CrashRecoveryTagProvider,
        marmosa::projections::NoopClock,
    > = StorageBackendProjectionStore::new_with_tag_provider(
        Arc::clone(&storage),
        "CrashRecoveryBalanceTagged".to_string(),
        CrashRecoveryTagProvider,
    );

    let tagged_projection = TaggedCrashRecoveryProjection;
    let runner = ProjectionRunner::new(Arc::clone(&storage), tagged_projection, projection_store);
    runner.rebuild(&store).await.unwrap();

    let read_store: StorageBackendProjectionStore<
        _,
        CrashRecoveryState,
        CrashRecoveryTagProvider,
        marmosa::projections::NoopClock,
    > = StorageBackendProjectionStore::new_with_tag_provider(
        Arc::clone(&storage),
        "CrashRecoveryBalanceTagged".to_string(),
        CrashRecoveryTagProvider,
    );

    let active = read_store
        .query_by_tag(&Tag {
            key: "status".to_string(),
            value: "active".to_string(),
        })
        .await
        .unwrap();
    assert_eq!(active.len(), 2);
}

struct TaggedCrashRecoveryProjection;

impl ProjectionDefinition for TaggedCrashRecoveryProjection {
    type State = CrashRecoveryState;

    fn projection_name(&self) -> &str {
        "CrashRecoveryBalanceTagged"
    }

    fn event_types(&self) -> Query {
        CrashRecoveryProjection.event_types()
    }

    fn key_selector(&self, event: &EventRecord) -> Option<String> {
        CrashRecoveryProjection.key_selector(event)
    }

    fn apply(&self, state: Option<Self::State>, event: &EventRecord) -> Option<Self::State> {
        CrashRecoveryProjection.apply(state, event)
    }
}