use marmosa::domain::Tag;


use std::sync::Arc;
use marmosa::projections::{ProjectionStore, StorageBackendProjectionStore, ProjectionTagProvider, NoopProjectionTagProvider};
use serde::{Deserialize, Serialize};

mod common;
use common::InMemoryStorage;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
struct TestProjection {
    pub id: String,
    pub status: String,
    pub tier: String,
}

struct TestProjectionTagProvider;

impl ProjectionTagProvider<TestProjection> for TestProjectionTagProvider {
    fn get_tags(&self, state: &TestProjection) -> Vec<Tag> {
        vec![
            Tag { key: "Status".to_string(), value: state.status.clone() },
            Tag { key: "Tier".to_string(), value: state.tier.clone() },
        ]
    }
}

async fn get_store() -> StorageBackendProjectionStore<Arc<InMemoryStorage>, TestProjection, TestProjectionTagProvider> {
    let _clock = common::FakeClock::new(0);
    let storage = Arc::new(InMemoryStorage::new());
    StorageBackendProjectionStore::new_with_tag_provider(storage, "TestProjection".to_string(), TestProjectionTagProvider)
}

#[tokio::test]
async fn query_by_tag_async_returns_projections_matching_tag_async() {
    let store = get_store().await;
    
    let proj1 = TestProjection { id: "1".to_string(), status: "Active".to_string(), tier: "Premium".to_string() };
    let proj2 = TestProjection { id: "2".to_string(), status: "Active".to_string(), tier: "Basic".to_string() };
    let proj3 = TestProjection { id: "3".to_string(), status: "Inactive".to_string(), tier: "Premium".to_string() };
    
    store.save("1", &proj1).await.unwrap();
    store.save("2", &proj2).await.unwrap();
    store.save("3", &proj3).await.unwrap();
    
    let results = store.query_by_tag(&Tag { key: "Status".to_string(), value: "Active".to_string() }).await.unwrap();
    
    assert_eq!(results.len(), 2);
    assert!(results.iter().any(|p| p.id == "1"));
    assert!(results.iter().any(|p| p.id == "2"));
}

#[tokio::test]
async fn query_by_tags_async_returns_projections_matching_all_tags_async() {
    let store = get_store().await;

    let proj1 = TestProjection { id: "1".to_string(), status: "Active".to_string(), tier: "Premium".to_string() };
    let proj2 = TestProjection { id: "2".to_string(), status: "Active".to_string(), tier: "Basic".to_string() };
    let proj3 = TestProjection { id: "3".to_string(), status: "Inactive".to_string(), tier: "Premium".to_string() };
    
    store.save("1", &proj1).await.unwrap();
    store.save("2", &proj2).await.unwrap();
    store.save("3", &proj3).await.unwrap();

    let tags = vec![Tag { key: "Status".to_string(), value: "Active".to_string() }, Tag { key: "Tier".to_string(), value: "Premium".to_string() }];
    let results = store.query_by_tags(&tags).await.unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "1");
}

#[tokio::test]
async fn query_by_tags_async_and_logic_is_correct_when_first_tag_index_is_larger_than_second_async() {
    let store = get_store().await;

    let proj1 = TestProjection { id: "1".to_string(), status: "Active".to_string(), tier: "Professional".to_string() };
    let proj2 = TestProjection { id: "2".to_string(), status: "Active".to_string(), tier: "Basic".to_string() };
    let proj3 = TestProjection { id: "3".to_string(), status: "Inactive".to_string(), tier: "Professional".to_string() };
    let proj4 = TestProjection { id: "4".to_string(), status: "Inactive".to_string(), tier: "Professional".to_string() };
    let proj5 = TestProjection { id: "5".to_string(), status: "Inactive".to_string(), tier: "Professional".to_string() };
    
    store.save("1", &proj1).await.unwrap();
    store.save("2", &proj2).await.unwrap();
    store.save("3", &proj3).await.unwrap();
    store.save("4", &proj4).await.unwrap();
    store.save("5", &proj5).await.unwrap();

    let tags = vec![Tag { key: "Tier".to_string(), value: "Professional".to_string() }, Tag { key: "Status".to_string(), value: "Active".to_string() }];
    let results = store.query_by_tags(&tags).await.unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "1");
}

#[tokio::test]
async fn query_by_tag_async_with_case_insensitive_comparison_finds_matches_async() {
    let store = get_store().await;

    let proj1 = TestProjection { id: "1".to_string(), status: "Active".to_string(), tier: "Premium".to_string() };
    store.save("1", &proj1).await.unwrap();

    let results = store.query_by_tag(&Tag { key: "status".to_string(), value: "active".to_string() }).await.unwrap();
    
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "1");
}

#[tokio::test]
async fn save_async_updates_indices_when_tags_change_async() {
    let store = get_store().await;

    let mut proj1 = TestProjection { id: "1".to_string(), status: "Pending".to_string(), tier: "Basic".to_string() };
    store.save("1", &proj1).await.unwrap();

    proj1.status = "Active".to_string();
    proj1.tier = "Premium".to_string();
    store.save("1", &proj1).await.unwrap();

    let active_results = store.query_by_tag(&Tag { key: "Status".to_string(), value: "Active".to_string() }).await.unwrap();
    assert_eq!(active_results.len(), 1);

    let premium_results = store.query_by_tag(&Tag { key: "Tier".to_string(), value: "Premium".to_string() }).await.unwrap();
    assert_eq!(premium_results.len(), 1);

    let pending_results = store.query_by_tag(&Tag { key: "Status".to_string(), value: "Pending".to_string() }).await.unwrap();
    assert_eq!(pending_results.len(), 0);

    let basic_results = store.query_by_tag(&Tag { key: "Tier".to_string(), value: "Basic".to_string() }).await.unwrap();
    assert_eq!(basic_results.len(), 0);
}

#[tokio::test]
async fn delete_async_removes_from_indices_async() {
    let store = get_store().await;

    let proj1 = TestProjection { id: "1".to_string(), status: "Active".to_string(), tier: "Premium".to_string() };
    store.save("1", &proj1).await.unwrap();

    store.delete("1").await.unwrap();

    let results = store.query_by_tag(&Tag { key: "Status".to_string(), value: "Active".to_string() }).await.unwrap();
    assert_eq!(results.len(), 0);
}

#[tokio::test]
async fn query_by_tag_async_without_tag_provider_returns_empty_async() {
    let _clock = common::FakeClock::new(0);
    let storage = Arc::new(InMemoryStorage::new());
    let store: StorageBackendProjectionStore<Arc<InMemoryStorage>, TestProjection, NoopProjectionTagProvider> =
        StorageBackendProjectionStore::new(storage, "TestProjection".to_string());
        
    let proj1 = TestProjection { id: "1".to_string(), status: "Active".to_string(), tier: "Premium".to_string() };
    store.save("1", &proj1).await.unwrap();

    let results = store.query_by_tag(&Tag { key: "Status".to_string(), value: "Active".to_string() }).await.unwrap();
    assert_eq!(results.len(), 0);
}
