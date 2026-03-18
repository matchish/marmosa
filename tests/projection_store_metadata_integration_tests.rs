mod common;

use common::InMemoryStorage;
use marmosa::ports::{Clock, StorageBackend};
use marmosa::projections::{
    NoopProjectionTagProvider, ProjectionMetadata, ProjectionStore, StorageBackendProjectionStore,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Default)]
pub struct TestProjection {
    pub id: String,
    pub value: String,
}

#[derive(Clone)]
pub struct FakeClock {
    pub now: Arc<Mutex<u64>>,
}

impl FakeClock {
    pub fn new(start: u64) -> Self {
        Self {
            now: Arc::new(Mutex::new(start)),
        }
    }

    pub fn advance(&self, millis: u64) {
        let mut n = self.now.lock().unwrap();
        *n += millis;
    }
}

impl Clock for FakeClock {
    fn now_millis(&self) -> u64 {
        *self.now.lock().unwrap()
    }
}

async fn create_store_and_storage(
    clock: Option<FakeClock>,
) -> (
    Arc<InMemoryStorage>,
    StorageBackendProjectionStore<
        Arc<InMemoryStorage>,
        TestProjection,
        NoopProjectionTagProvider,
        FakeClock,
    >,
) {
    let storage = InMemoryStorage::new();
    let arc_storage = Arc::new(storage);

    let store = StorageBackendProjectionStore::new_with_tag_provider_and_clock(
        arc_storage.clone(),
        "TestProjection".to_string(),
        NoopProjectionTagProvider,
        clock,
    );

    (arc_storage, store)
}

#[tokio::test]
async fn save_async_creates_metadata_index_on_first_save() {
    let clock = FakeClock::new(1234567890);
    let (storage, store) = create_store_and_storage(Some(clock)).await;

    let projection = TestProjection {
        id: "test-1".to_string(),
        value: "Initial".to_string(),
    };

    store.save("test-1", &projection).await.unwrap();

    let metadata_file = "Projections/TestProjection/Metadata/index.json";
    let data = storage.read_file(metadata_file).await;
    assert!(data.is_ok());
}

#[tokio::test]
async fn save_async_wraps_projection_with_metadata() {
    let clock = FakeClock::new(1234567890);
    let (storage, store) = create_store_and_storage(Some(clock)).await;

    let projection = TestProjection {
        id: "test-1".to_string(),
        value: "Test".to_string(),
    };

    store.save("test-1", &projection).await.unwrap();

    let file_path = "Projections/TestProjection/test-1.json";
    let data = storage.read_file(file_path).await.unwrap();
    let json_str = String::from_utf8(data).unwrap();

    assert!(json_str.contains(r#""data""#));
    assert!(json_str.contains(r#""metadata""#));
    assert!(json_str.contains(r#""createdAt""#));
    assert!(json_str.contains(r#""lastUpdatedAt""#));
    assert!(json_str.contains(r#""version""#));
    assert!(json_str.contains(r#""sizeInBytes""#));
}

#[tokio::test]
async fn get_async_unwraps_metadata() {
    let clock = FakeClock::new(1234567890);
    let (_storage, store) = create_store_and_storage(Some(clock)).await;

    let projection = TestProjection {
        id: "test-1".to_string(),
        value: "Test".to_string(),
    };
    store.save("test-1", &projection).await.unwrap();

    let retrieved = store.get("test-1").await.unwrap().unwrap();

    assert_eq!(retrieved.id, "test-1");
    assert_eq!(retrieved.value, "Test");
}

#[tokio::test]
async fn save_async_increments_version_on_update() {
    let clock = FakeClock::new(1234567890);
    let (storage, store) = create_store_and_storage(Some(clock)).await;
    let index_path = "Projections/TestProjection/Metadata/index.json";

    store
        .save(
            "test-1",
            &TestProjection {
                id: "test-1".to_string(),
                value: "V1".to_string(),
            },
        )
        .await
        .unwrap();
    let data1 = storage.read_file(index_path).await.unwrap();
    let index1: BTreeMap<String, ProjectionMetadata> = serde_json::from_slice(&data1).unwrap();

    store
        .save(
            "test-1",
            &TestProjection {
                id: "test-1".to_string(),
                value: "V2".to_string(),
            },
        )
        .await
        .unwrap();
    let data2 = storage.read_file(index_path).await.unwrap();
    let index2: BTreeMap<String, ProjectionMetadata> = serde_json::from_slice(&data2).unwrap();

    store
        .save(
            "test-1",
            &TestProjection {
                id: "test-1".to_string(),
                value: "V3".to_string(),
            },
        )
        .await
        .unwrap();
    let data3 = storage.read_file(index_path).await.unwrap();
    let index3: BTreeMap<String, ProjectionMetadata> = serde_json::from_slice(&data3).unwrap();

    assert_eq!(index1.get("test-1").unwrap().version, 1);
    assert_eq!(index2.get("test-1").unwrap().version, 2);
    assert_eq!(index3.get("test-1").unwrap().version, 3);
}

#[tokio::test]
async fn save_async_updates_last_updated_at() {
    let clock = FakeClock::new(100);
    let (storage, store) = create_store_and_storage(Some(clock.clone())).await;
    let index_path = "Projections/TestProjection/Metadata/index.json";

    store
        .save(
            "test-1",
            &TestProjection {
                id: "test-1".to_string(),
                value: "V1".to_string(),
            },
        )
        .await
        .unwrap();
    let data1 = storage.read_file(index_path).await.unwrap();
    let index1: BTreeMap<String, ProjectionMetadata> = serde_json::from_slice(&data1).unwrap();

    clock.advance(100);

    store
        .save(
            "test-1",
            &TestProjection {
                id: "test-1".to_string(),
                value: "V2".to_string(),
            },
        )
        .await
        .unwrap();
    let data2 = storage.read_file(index_path).await.unwrap();
    let index2: BTreeMap<String, ProjectionMetadata> = serde_json::from_slice(&data2).unwrap();

    assert!(
        index2.get("test-1").unwrap().last_updated_at
            > index1.get("test-1").unwrap().last_updated_at
    );
}

#[tokio::test]
async fn save_async_maintains_created_at() {
    let clock = FakeClock::new(100);
    let (storage, store) = create_store_and_storage(Some(clock.clone())).await;
    let index_path = "Projections/TestProjection/Metadata/index.json";

    store
        .save(
            "test-1",
            &TestProjection {
                id: "test-1".to_string(),
                value: "V1".to_string(),
            },
        )
        .await
        .unwrap();
    let data1 = storage.read_file(index_path).await.unwrap();
    let index1: BTreeMap<String, ProjectionMetadata> = serde_json::from_slice(&data1).unwrap();

    clock.advance(100);

    store
        .save(
            "test-1",
            &TestProjection {
                id: "test-1".to_string(),
                value: "V2".to_string(),
            },
        )
        .await
        .unwrap();
    let data2 = storage.read_file(index_path).await.unwrap();
    let index2: BTreeMap<String, ProjectionMetadata> = serde_json::from_slice(&data2).unwrap();

    assert_eq!(
        index1.get("test-1").unwrap().created_at,
        index2.get("test-1").unwrap().created_at
    );
}

#[tokio::test]
async fn save_async_updates_size_in_bytes() {
    let clock = FakeClock::new(100);
    let (storage, store) = create_store_and_storage(Some(clock)).await;
    let index_path = "Projections/TestProjection/Metadata/index.json";

    store
        .save(
            "test-1",
            &TestProjection {
                id: "test-1".to_string(),
                value: "Small".to_string(),
            },
        )
        .await
        .unwrap();
    let data1 = storage.read_file(index_path).await.unwrap();
    let index1: BTreeMap<String, ProjectionMetadata> = serde_json::from_slice(&data1).unwrap();

    store
        .save(
            "test-1",
            &TestProjection {
                id: "test-1".to_string(),
                value: "Much larger value that will increase the JSON size significantly"
                    .to_string(),
            },
        )
        .await
        .unwrap();
    let data2 = storage.read_file(index_path).await.unwrap();
    let index2: BTreeMap<String, ProjectionMetadata> = serde_json::from_slice(&data2).unwrap();

    assert!(
        index2.get("test-1").unwrap().size_in_bytes > index1.get("test-1").unwrap().size_in_bytes
    );
}

#[tokio::test]
async fn delete_async_removes_metadata() {
    let clock = FakeClock::new(100);
    let (storage, store) = create_store_and_storage(Some(clock)).await;
    let index_path = "Projections/TestProjection/Metadata/index.json";

    store
        .save(
            "test-1",
            &TestProjection {
                id: "test-1".to_string(),
                value: "Test".to_string(),
            },
        )
        .await
        .unwrap();

    store.delete("test-1").await.unwrap();

    let data = storage.read_file(index_path).await.unwrap();
    let index: BTreeMap<String, ProjectionMetadata> = serde_json::from_slice(&data).unwrap();

    assert!(!index.contains_key("test-1"));
}
