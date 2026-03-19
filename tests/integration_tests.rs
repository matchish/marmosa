mod common;

use common::{FakeClock, InMemoryStorage};
use marmosa::domain::{DomainEvent, EventData, EventRecord, Query, QueryItem};
use marmosa::event_store::{EventStore, MarmosaStore};
use marmosa::projections::{
    ProjectionDefinition, ProjectionRunner, ProjectionStore, StorageBackendProjectionStore,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct CounterState {
    pub count: i32,
    pub last_event: String,
}

impl Default for CounterState {
    fn default() -> Self {
        Self {
            count: 0,
            last_event: "".to_string(),
        }
    }
}

pub struct CounterProjection;

impl ProjectionDefinition for CounterProjection {
    type State = CounterState;

    fn projection_name(&self) -> &str {
        "CounterProjection"
    }

    fn event_types(&self) -> Query {
        Query {
            items: vec![QueryItem {
                event_types: vec!["CounterIncremented".to_string()],
                tags: vec![],
            }],
        }
    }

    fn key_selector(&self, event: &EventRecord) -> Option<String> {
        let stream_id = event.event_id.split('-').next().unwrap_or("unknown");
        Some(stream_id.to_string())
    }

    fn apply(&self, state: Option<Self::State>, event: &EventRecord) -> Option<Self::State> {
        let mut state = state.unwrap_or_default();
        if event.event.event_type == "CounterIncremented" {
            state.count += 1;
            state.last_event = event.event_id.clone();
        }
        Some(state)
    }
}

#[tokio::test]
async fn test_eventstore_projections_end_to_end() {
    let storage = Arc::new(InMemoryStorage::new());
    let clock = FakeClock::new(1000);

    // Create event store
    let store = MarmosaStore::new(Arc::clone(&storage), clock);

    // Create projection store
    let projection_store =
        StorageBackendProjectionStore::new(Arc::clone(&storage), "CounterProjection".to_string());

    // Create runner
    let runner = ProjectionRunner::new(Arc::clone(&storage), CounterProjection, projection_store);

    // 1. Append CounterIncremented events
    let event1 = EventData {
        event_id: "stream1-1".to_string(),
        event: DomainEvent {
            event_type: "CounterIncremented".to_string(),
            data: r#"{"val": 1}"#.to_string(),
            tags: vec![],
        },
        metadata: None,
    };

    let event2 = EventData {
        event_id: "stream2-1".to_string(),
        event: DomainEvent {
            event_type: "CounterIncremented".to_string(),
            data: r#"{"val": 2}"#.to_string(),
            tags: vec![],
        },
        metadata: None,
    };

    store.append_async(vec![event1], None).await.unwrap();
    store.append_async(vec![event2], None).await.unwrap();

    // Read events to feed to runner
    let all_events = store
        .read_async(Query::all(), None, None, None)
        .await
        .unwrap();

    // 2. Run projection from scratch
    runner.process_events(&all_events).await.unwrap();

    // 3. Verify it was saved properly in projection store
    // Let's get the store again
    let p_store =
        StorageBackendProjectionStore::new(Arc::clone(&storage), "CounterProjection".to_string());
    let state1: Option<CounterState> = p_store.get("stream1").await.unwrap();
    assert!(state1.is_some());
    assert_eq!(state1.unwrap().count, 1);

    let state2: Option<CounterState> = p_store.get("stream2").await.unwrap();
    assert!(state2.is_some());
    assert_eq!(state2.unwrap().count, 1);

    let checkpoint = runner.get_checkpoint().await.unwrap();
    assert!(checkpoint.is_some());

    // 4. Append more events to stream 1
    let event3 = EventData {
        event_id: "stream1-2".to_string(),
        event: DomainEvent {
            event_type: "CounterIncremented".to_string(),
            data: r#"{"val": 3}"#.to_string(),
            tags: vec![],
        },
        metadata: None,
    };
    store.append_async(vec![event3], None).await.unwrap();

    // Read events again from the checkpoint instead of all
    let checkpoint = runner.get_checkpoint().await.unwrap();
    let start_pos = checkpoint.map(|c| c.last_position);
    let new_events = store
        .read_async(Query::all(), start_pos, None, None)
        .await
        .unwrap();

    // 5. Run projection again (should resume from checkpoint internally skipping early ones)
    runner.process_events(&new_events).await.unwrap();

    // Verify stream1 state updated
    let state1_resumed: Option<CounterState> = p_store.get("stream1").await.unwrap();
    assert_eq!(state1_resumed.unwrap().count, 2);
}
