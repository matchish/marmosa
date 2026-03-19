mod common;

use common::{FakeClock, InMemoryStorage};
use marmosa::domain::{DomainEvent, EventData, Query};
use marmosa::event_store::{EventStore, MarmosaStore};
use std::sync::Arc;

type TestStore = MarmosaStore<Arc<InMemoryStorage>, FakeClock>;

struct MarmosaFixture {
    event_store: Arc<TestStore>,
}

impl MarmosaFixture {
    fn new() -> Self {
        let storage = Arc::new(InMemoryStorage::new());
        let store = MarmosaStore::new(storage, FakeClock::new(0));
        Self {
            event_store: Arc::new(store),
        }
    }

    fn event_store(&self) -> Arc<TestStore> {
        Arc::clone(&self.event_store)
    }
}

fn sample_event(event_id: &str) -> EventData {
    EventData {
        event_id: event_id.to_string(),
        event: DomainEvent {
            event_type: "FixtureEvent".to_string(),
            data: "{}".to_string(),
            tags: vec![],
        },
        metadata: None,
    }
}

#[tokio::test]
async fn constructor_initializes_event_store() {
    let fixture = MarmosaFixture::new();
    let store = fixture.event_store();

    store.append_async(vec![sample_event("event-1")], None).await.unwrap();
    let events = store.read_async(Query::all(), None, None, None).await.unwrap();

    assert_eq!(events.len(), 1);
}

#[tokio::test]
async fn constructor_creates_unique_store_instance_per_fixture() {
    let fixture1 = MarmosaFixture::new();
    let fixture2 = MarmosaFixture::new();

    let store1 = fixture1.event_store();
    let store2 = fixture2.event_store();

    assert!(!Arc::ptr_eq(&store1, &store2));
}

#[tokio::test]
async fn fixture_provides_same_store_for_lifetime() {
    let fixture = MarmosaFixture::new();

    let store1 = fixture.event_store();
    let store2 = fixture.event_store();

    assert!(Arc::ptr_eq(&store1, &store2));
}

#[tokio::test]
async fn event_store_can_be_used_for_event_operations() {
    let fixture = MarmosaFixture::new();
    let store = fixture.event_store();

    store
        .append_async(vec![sample_event("event-operations")], None)
        .await
        .unwrap();

    let events = store.read_async(Query::all(), None, None, None).await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event.event_type, "FixtureEvent");
}

#[tokio::test]
async fn fixture_isolates_data_between_instances() {
    let fixture1 = MarmosaFixture::new();
    let fixture2 = MarmosaFixture::new();

    let store1 = fixture1.event_store();
    let store2 = fixture2.event_store();

    store1
        .append_async(vec![sample_event("fixture-1-only")], None)
        .await
        .unwrap();

    let events1 = store1.read_async(Query::all(), None, None, None).await.unwrap();
    let events2 = store2.read_async(Query::all(), None, None, None).await.unwrap();

    assert_eq!(events1.len(), 1);
    assert!(events2.is_empty());
}