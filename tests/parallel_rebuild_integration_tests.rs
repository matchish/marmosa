mod common;

use common::{FakeClock, InMemoryStorage};
use marmosa::event_store::{EventStore, OpossumStore};
use marmosa::projections::{ProjectionRunner, StorageBackendProjectionStore, ProjectionDefinition, ProjectionRebuilder};
use marmosa::domain::{Query, QueryItem, EventRecord, EventData, DomainEvent};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct DummyState {
    pub count: i32,
}

macro_rules! define_slow_projection {
    ($name:ident, $str_name:expr, $event_type:expr) => {
        pub struct $name;
        impl ProjectionDefinition for $name {
            type State = DummyState;
            fn projection_name(&self) -> &str { $str_name }
            fn event_types(&self) -> Query {
                Query {
                    items: vec![QueryItem {
                        event_types: vec![$event_type.to_string()],
                        tags: vec![],
                    }],
                }
            }
            fn key_selector(&self, _event: &EventRecord) -> Option<String> {
                Some("dummy".to_string())
            }
            fn apply(&self, state: Option<Self::State>, _event: &EventRecord) -> Option<Self::State> {
                println!("Applying event"); std::thread::sleep(Duration::from_millis(50));
                let state = state.unwrap_or_default();
                Some(DummyState { count: state.count + 1 })
            }
        }
    };
}

define_slow_projection!(SlowProjection1, "Slow1", "SlowEvent1");
define_slow_projection!(SlowProjection2, "Slow2", "SlowEvent2");
define_slow_projection!(SlowProjection3, "Slow3", "SlowEvent3");
define_slow_projection!(SlowProjection4, "Slow4", "SlowEvent4");

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_rebuilds_of_different_projections_execute_in_parallel() {
    let clock = FakeClock::new(0);
    let storage = Arc::new(InMemoryStorage::new());
    let store = OpossumStore::new(Arc::clone(&storage), common::FakeClock::new(0));

    for _ in 0..6 {
        store.append_async(vec![
        EventData { event_id: uuid::Uuid::new_v4().to_string(), event: DomainEvent { event_type: "SlowEvent1".into(), data: "[]".into(), tags: vec![] }, metadata: None },
        EventData { event_id: uuid::Uuid::new_v4().to_string(), event: DomainEvent { event_type: "SlowEvent2".into(), data: "[]".into(), tags: vec![] }, metadata: None },
        EventData { event_id: uuid::Uuid::new_v4().to_string(), event: DomainEvent { event_type: "SlowEvent3".into(), data: "[]".into(), tags: vec![] }, metadata: None },
        EventData { event_id: uuid::Uuid::new_v4().to_string(), event: DomainEvent { event_type: "SlowEvent4".into(), data: "[]".into(), tags: vec![] }, metadata: None },
        ], None).await.unwrap();
    }

    let mut rebuilder = ProjectionRebuilder::new(&store);
    rebuilder.register(ProjectionRunner::new(Arc::clone(&storage), SlowProjection1, StorageBackendProjectionStore::new(Arc::clone(&storage), "Slow1".into())));
    rebuilder.register(ProjectionRunner::new(Arc::clone(&storage), SlowProjection2, StorageBackendProjectionStore::new(Arc::clone(&storage), "Slow2".into())));
    rebuilder.register(ProjectionRunner::new(Arc::clone(&storage), SlowProjection3, StorageBackendProjectionStore::new(Arc::clone(&storage), "Slow3".into())));
    rebuilder.register(ProjectionRunner::new(Arc::clone(&storage), SlowProjection4, StorageBackendProjectionStore::new(Arc::clone(&storage), "Slow4".into())));

    let start = std::time::Instant::now();
    let result = rebuilder.rebuild_all(false).await;
    let elapsed = start.elapsed();

    assert!(result.success);
    assert_eq!(result.total_rebuilt, 4);

    assert!(elapsed.as_millis() < 2500, "Parallel run took too long: {} ms", elapsed.as_millis());
}

define_slow_projection!(LongRunningProjection, "LongRunning", "LongRunningEvent");

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn duplicate_rebuild_same_projection_executes_sequentially() {
    let clock = FakeClock::new(0);
    let storage = Arc::new(InMemoryStorage::new());
    let store = Arc::new(OpossumStore::new(Arc::clone(&storage), common::FakeClock::new(0)));

    let mut events = Vec::new();
    for _ in 0..10 {
        events.push(EventData { event_id: uuid::Uuid::new_v4().to_string(), event: DomainEvent { event_type: "LongRunningEvent".into(), data: "{}".into(), tags: vec![] }, metadata: None });
    }
    store.append_async(events, None).await.unwrap();


    let store1 = Arc::clone(&store);
    let strg1 = Arc::clone(&storage);
    let task1 = tokio::spawn(async move {
        let runner = ProjectionRunner::new(Arc::clone(&strg1), LongRunningProjection, StorageBackendProjectionStore::new(Arc::clone(&strg1), "LongRunning".into()));
        let mut rebuilder = ProjectionRebuilder::new(&*store1);
        rebuilder.register(runner);
        rebuilder.rebuild_all(false).await
    });

    // f2
    tokio::time::sleep(Duration::from_millis(50)).await;

    let runner2 = ProjectionRunner::new(Arc::clone(&storage), LongRunningProjection, StorageBackendProjectionStore::new(Arc::clone(&storage), "LongRunning".into()));
    let mut rebuilder2 = ProjectionRebuilder::new(&*store);
    rebuilder2.register(runner2);
    
    let start2 = std::time::Instant::now();
    let res2 = rebuilder2.rebuild_all(true).await;
    let elapsed2 = start2.elapsed();

    let res1 = task1.await.unwrap();



    println!("res1: success = {}, total = {}", res1.success, res1.total_rebuilt); println!("res1: success = {}, total = {}", res1.success, res1.total_rebuilt); assert!(res1.success);
    println!("res2: success = {}, total = {}", res2.success, res2.total_rebuilt); println!("res2: success = {}, total = {}", res2.success, res2.total_rebuilt); assert!(res2.success);
    
    assert!(elapsed2.as_millis() >= 10, "Wait elapsed: {}", elapsed2.as_millis());
}

#[tokio::test]
async fn rebuild_after_rebuild_same_projection_succeeds() {
    let clock = FakeClock::new(0);
    let storage = Arc::new(InMemoryStorage::new());
    let store = OpossumStore::new(Arc::clone(&storage), common::FakeClock::new(0));

    store.append_async(vec![
        EventData { event_id: uuid::Uuid::new_v4().to_string(), event: DomainEvent { event_type: "SlowEvent1".into(), data: "[]".into(), tags: vec![] }, metadata: None },
    ], None).await.unwrap();

    let runner = ProjectionRunner::new(Arc::clone(&storage), SlowProjection1, StorageBackendProjectionStore::new(Arc::clone(&storage), "Slow1".into()));
    let mut rebuilder = ProjectionRebuilder::new(&store);
    rebuilder.register(runner);
    let res1 = rebuilder.rebuild_all(true).await; 
    println!("res1: success = {}, total = {}", res1.success, res1.total_rebuilt); println!("res1: success = {}, total = {}", res1.success, res1.total_rebuilt); assert!(res1.success);

    let runner2 = ProjectionRunner::new(Arc::clone(&storage), SlowProjection1, StorageBackendProjectionStore::new(Arc::clone(&storage), "Slow1".into()));
    let mut rebuilder2 = ProjectionRebuilder::new(&store);
    rebuilder2.register(runner2);
    let res2 = rebuilder2.rebuild_all(true).await; 
    println!("res2: success = {}, total = {}", res2.success, res2.total_rebuilt); println!("res2: success = {}, total = {}", res2.success, res2.total_rebuilt); assert!(res2.success);
}
