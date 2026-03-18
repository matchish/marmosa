mod common;

use common::{FakeClock, InMemoryStorage};
use marmosa::event_store::OpossumStore;
use marmosa::projections::{ProjectionRunner, StorageBackendProjectionStore, ProjectionDefinition, ProjectionRebuilder};
use marmosa::domain::{Query, QueryItem, EventRecord};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct DummyState {
    pub count: i32,
}

pub struct DummyProjection1;
impl ProjectionDefinition for DummyProjection1 {
    type State = DummyState;
    fn projection_name(&self) -> &str { "TestProjection1" }
    fn event_types(&self) -> Query {
        Query {
            items: vec![QueryItem {
                event_types: vec!["TestEvent".to_string()],
                tags: vec![],
            }],
        }
    }
    fn key_selector(&self, _event: &EventRecord) -> Option<String> {
        Some("dummy".to_string())
    }
    fn apply(&self, state: Option<Self::State>, _event: &EventRecord) -> Option<Self::State> {
        let state = state.unwrap_or_default();
        Some(DummyState { count: state.count + 1 })
    }
}

pub struct DummyProjection2;
impl ProjectionDefinition for DummyProjection2 {
    type State = DummyState;
    fn projection_name(&self) -> &str { "TestProjection2" }
    fn event_types(&self) -> Query {
        Query {
            items: vec![QueryItem {
                event_types: vec!["TestEvent".to_string()],
                tags: vec![],
            }],
        }
    }
    fn key_selector(&self, _event: &EventRecord) -> Option<String> {
        Some("dummy".to_string())
    }
    fn apply(&self, state: Option<Self::State>, _event: &EventRecord) -> Option<Self::State> {
        let state = state.unwrap_or_default();
        Some(DummyState { count: state.count + 1 })
    }
}

#[tokio::test]
async fn test_parallel_rebuild_success() {
    let clock = FakeClock::new(0);
    let storage1 = Arc::new(InMemoryStorage::new());
    let storage2 = Arc::new(InMemoryStorage::new());
    let store = OpossumStore::new(Arc::clone(&storage1), FakeClock::new(0));

    let pstore1 = StorageBackendProjectionStore::new(Arc::clone(&storage1), String::from("TestProjection1"));
    let pstore2 = StorageBackendProjectionStore::new(Arc::clone(&storage2), String::from("TestProjection2"));

    let runner1 = ProjectionRunner::new(Arc::clone(&storage1), DummyProjection1, pstore1);
    let runner2 = ProjectionRunner::new(Arc::clone(&storage2), DummyProjection2, pstore2);

    let mut rebuilder = ProjectionRebuilder::new(&store);
    rebuilder.register(runner1);
    rebuilder.register(runner2);

    let result = rebuilder.rebuild_all(false).await;
    assert!(result.success);
    assert_eq!(result.total_rebuilt, 2);
}

