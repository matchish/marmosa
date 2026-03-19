mod common;

use common::{FakeClock, InMemoryStorage};
use marmosa::domain::{DomainEvent, EventData, EventRecord, Query, QueryItem};
use marmosa::event_store::{EventStore, OpossumStore};
use marmosa::projections::{
    AutoRebuildMode, ProjectionDefinition, ProjectionOptions, ProjectionRebuilder,
    ProjectionRunner, ProjectionStore, StorageBackendProjectionStore,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct E2ETestState {
    count: i32,
}

struct E2ETestProjection;

impl ProjectionDefinition for E2ETestProjection {
    type State = E2ETestState;

    fn projection_name(&self) -> &str {
        "E2ETest"
    }

    fn event_types(&self) -> Query {
        Query {
            items: vec![QueryItem {
                event_types: vec!["TestEvent".to_string()],
                tags: vec![],
            }],
        }
    }

    fn key_selector(&self, _event: &EventRecord) -> Option<String> {
        Some("key".to_string())
    }

    fn apply(&self, state: Option<Self::State>, _event: &EventRecord) -> Option<Self::State> {
        let mut state = state.unwrap_or_default();
        state.count += 1;
        Some(state)
    }
}

#[test]
fn projection_options_customization_applies_configuration() {
    let options = ProjectionOptions {
        polling_interval: Duration::from_secs(10),
        batch_size: 500,
        auto_rebuild: AutoRebuildMode::None,
        ..Default::default()
    };

    assert_eq!(options.polling_interval, Duration::from_secs(10));
    assert_eq!(options.batch_size, 500);
    assert_eq!(options.auto_rebuild, AutoRebuildMode::None);
}

#[tokio::test]
async fn explicit_registration_builds_runner_and_projection_store() {
    let storage = Arc::new(InMemoryStorage::new());
    let store = OpossumStore::new(Arc::clone(&storage), FakeClock::new(0));

    store
        .append_async(
            vec![EventData {
                event_id: uuid::Uuid::new_v4().to_string(),
                event: DomainEvent {
                    event_type: "TestEvent".to_string(),
                    data: "{}".to_string(),
                    tags: vec![],
                },
                metadata: None,
            }],
            None,
        )
        .await
        .unwrap();

    let projection_store =
        StorageBackendProjectionStore::new(Arc::clone(&storage), "E2ETest".to_string());
    let runner = ProjectionRunner::new(Arc::clone(&storage), E2ETestProjection, projection_store);

    let events = store.read_async(Query::all(), None, None, None).await.unwrap();
    runner.process_events(&events).await.unwrap();

    let result_store: StorageBackendProjectionStore<_, E2ETestState> =
        StorageBackendProjectionStore::new(Arc::clone(&storage), "E2ETest".to_string());
    let state = result_store.get("key").await.unwrap();

    assert!(state.is_some());
    assert_eq!(state.unwrap().count, 1);
}

#[tokio::test]
async fn projection_rebuilder_runs_registered_projection() {
    let storage = Arc::new(InMemoryStorage::new());
    let store = OpossumStore::new(Arc::clone(&storage), FakeClock::new(0));

    for _ in 0..3 {
        store
            .append_async(
                vec![EventData {
                    event_id: uuid::Uuid::new_v4().to_string(),
                    event: DomainEvent {
                        event_type: "TestEvent".to_string(),
                        data: "{}".to_string(),
                        tags: vec![],
                    },
                    metadata: None,
                }],
                None,
            )
            .await
            .unwrap();
    }

    let projection_store =
        StorageBackendProjectionStore::new(Arc::clone(&storage), "E2ETest".to_string());
    let runner = ProjectionRunner::new(Arc::clone(&storage), E2ETestProjection, projection_store);

    let mut rebuilder = ProjectionRebuilder::new(&store);
    rebuilder.register(runner);

    let result = rebuilder.rebuild_all(false).await;
    assert!(result.success);
    assert_eq!(result.total_rebuilt, 1);
}

#[tokio::test]
async fn projection_rebuilder_without_registered_projections_is_noop_success() {
    let storage = Arc::new(InMemoryStorage::new());
    let store = OpossumStore::new(Arc::clone(&storage), FakeClock::new(0));

    let rebuilder = ProjectionRebuilder::new(&store);
    let result = rebuilder.rebuild_all(false).await;

    assert!(result.success);
    assert_eq!(result.total_rebuilt, 0);
    assert!(result.failed_projections.is_empty());
}