mod common;

use common::{FakeClock, InMemoryStorage};
use std::sync::Arc;
use serde::{Deserialize, Serialize};

use marmosa::domain::{DomainEvent, EventData, EventRecord, Query, Tag};
use marmosa::event_store::{EventStore, OpossumStore};
use marmosa::projections::{
    ProjectionDefinition, ProjectionRunner, ProjectionStore, StorageBackendProjectionStore
};

// -----------------------------------------------------------------------------
// Projection Definition
// -----------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct PmTestItemState {
    item_id: String,
    name: String,
}

#[derive(Serialize, Deserialize)]
struct PmTestItemCreated {
    item_id: String,
    name: String,
}

#[derive(Serialize, Deserialize)]
struct PmTestItemUpdated {
    item_id: String,
    name: String,
}

#[derive(Serialize, Deserialize)]
struct PmTestItemDeleted {
    item_id: String,
}

#[derive(Serialize, Deserialize)]
struct PmIrrelevantEvent {
    index: i32,
}

struct PmTestItemProjection;
impl ProjectionDefinition for PmTestItemProjection {
    type State = PmTestItemState;

    fn projection_name(&self) -> &str {
        "PmTestItems"
    }

    fn event_types(&self) -> Query {
        Query { items: vec![marmosa::domain::QueryItem { event_types: vec![
            "PmTestItemCreated".to_string(),
            "PmTestItemUpdated".to_string(),
            "PmTestItemDeleted".to_string(),
        ], tags: vec![] }] }
    }

    fn key_selector(&self, event: &EventRecord) -> Option<String> {
        event.event.tags.iter().find(|t| t.key == "itemId").map(|t| t.value.clone())
    }

    fn apply(&self, state: Option<Self::State>, event: &EventRecord) -> Option<Self::State> {
        match event.event.event_type.as_str() {
            "PmTestItemCreated" => {
                let data: PmTestItemCreated = serde_json::from_str(&event.event.data).unwrap();
                Some(PmTestItemState {
                    item_id: data.item_id,
                    name: data.name,
                })
            }
            "PmTestItemUpdated" => {
                let mut state = state.unwrap();
                let data: PmTestItemUpdated = serde_json::from_str(&event.event.data).unwrap();
                state.name = data.name;
                Some(state)
            }
            "PmTestItemDeleted" => None,
            _ => state,
        }
    }
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

fn create_event(
    event_type: &str,
    tags: Vec<(&str, &str)>,
    data: &impl Serialize,
) -> EventData {
    let mut mapped_tags = Vec::new();
    for (k, v) in tags {
        mapped_tags.push(Tag {
            key: k.to_string(),
            value: v.to_string(),
        });
    }
    
    EventData {
        event_id: uuid::Uuid::new_v4().to_string(),
        event: DomainEvent {
            event_type: event_type.to_string(),
            data: serde_json::to_string(data).unwrap(),
            tags: mapped_tags,
        },
        metadata: None,
    }
}

// =============================================================================
// Tests
// =============================================================================

#[tokio::test]
async fn rebuild_async_with_existing_events_builds_projection_async() {
    let storage = Arc::new(InMemoryStorage::new());
    let store = OpossumStore::new(Arc::clone(&storage), FakeClock::new(100));

    let proj = PmTestItemProjection;
    let store_proj_runner = StorageBackendProjectionStore::new(Arc::clone(&storage), "PmTestItems".to_string());
    let runner = ProjectionRunner::new(Arc::clone(&storage), proj, store_proj_runner);

    let item_id = "item-1";
    let ev = create_event(
        "PmTestItemCreated",
        vec![("itemId", item_id)],
        &PmTestItemCreated {
            item_id: item_id.to_string(),
            name: "Test Item".to_string(),
        },
    );
    store.append_async(vec![ev], None).await.unwrap();

    runner.rebuild(&store).await.unwrap();

    let store_proj = StorageBackendProjectionStore::new(Arc::clone(&storage), "PmTestItems".to_string());
    let item: PmTestItemState = store_proj.get(item_id).await.unwrap().unwrap();
    assert_eq!(item.item_id, item_id);
    assert_eq!(item.name, "Test Item");
}

#[tokio::test]
async fn rebuild_async_with_delete_event_removes_projection_async() {
    let storage = Arc::new(InMemoryStorage::new());
    let store = OpossumStore::new(Arc::clone(&storage), FakeClock::new(100));

    let proj = PmTestItemProjection;
    let store_proj_runner = StorageBackendProjectionStore::new(Arc::clone(&storage), "PmTestItems".to_string());
    let runner = ProjectionRunner::new(Arc::clone(&storage), proj, store_proj_runner);

    let item_id = "item-1";
    let ev1 = create_event(
        "PmTestItemCreated",
        vec![("itemId", item_id)],
        &PmTestItemCreated {
            item_id: item_id.to_string(),
            name: "Test Item".to_string(),
        },
    );
    let ev2 = create_event(
        "PmTestItemDeleted",
        vec![("itemId", item_id)],
        &PmTestItemDeleted {
            item_id: item_id.to_string(),
        },
    );
    store.append_async(vec![ev1, ev2], None).await.unwrap();

    runner.rebuild(&store).await.unwrap();

    let store_proj = StorageBackendProjectionStore::new(Arc::clone(&storage), "PmTestItems".to_string());
    let item: Option<PmTestItemState> = store_proj.get(item_id).await.unwrap();
    assert!(item.is_none());
}

#[tokio::test]
async fn rebuild_async_sparse_projection_checkpoint_advanced_to_store_head_async() {
    let storage = Arc::new(InMemoryStorage::new());
    let store = OpossumStore::new(Arc::clone(&storage), FakeClock::new(100));

    let proj = PmTestItemProjection;
    let store_proj_runner = StorageBackendProjectionStore::new(Arc::clone(&storage), "PmTestItems".to_string());
    let runner = ProjectionRunner::new(Arc::clone(&storage), proj, store_proj_runner);

    for i in 0..3 {
        let item_id = format!("item-{}", i);
        let ev = create_event(
            "PmTestItemCreated",
            vec![("itemId", &item_id)],
            &PmTestItemCreated {
                item_id: item_id.clone(),
                name: format!("Item {}", i),
            },
        );
        store.append_async(vec![ev], None).await.unwrap();
    }

    for i in 0..7 {
        let ev = create_event(
            "PmIrrelevantEvent",
            vec![],
            &PmIrrelevantEvent { index: i },
        );
        store.append_async(vec![ev], None).await.unwrap();
    }

    runner.rebuild(&store).await.unwrap();

    let cp = runner.get_checkpoint().await.unwrap().unwrap();
    assert_eq!(cp.last_position, 9);
    
    let store_proj = StorageBackendProjectionStore::new(Arc::clone(&storage), "PmTestItems".to_string());
    let all: Vec<PmTestItemState> = store_proj.get_all().await.unwrap();
    assert_eq!(all.len(), 3);
}
