import re

with open('src/event_store.rs', 'r') as f:
    text = f.read()

impls = """

pub struct IndexManager<S> {
    tag_index: TagIndex<S>,
    event_type_index: EventTypeIndex<S>,
}

impl<S: crate::ports::StorageBackend + Send + Sync + Clone> IndexManager<S> {
    pub fn new(storage: S) -> Self {
        Self {
            tag_index: TagIndex::new(storage.clone()),
            event_type_index: EventTypeIndex::new(storage),
        }
    }

    pub async fn add_event_to_indices_async(&self, root_path: &str, record: &crate::domain::EventRecord) -> Result<(), crate::ports::Error> {
        self.event_type_index.add_position_async(root_path, &record.event.event_type, record.position).await?;
        for tag in &record.event.tags {
            self.tag_index.add_position_async(root_path, tag, record.position).await?;
        }
        Ok(())
    }

    pub async fn get_positions_by_event_type_async(&self, root_path: &str, event_type: &str) -> Result<alloc::vec::Vec<u64>, crate::ports::Error> {
        self.event_type_index.get_positions_async(root_path, event_type).await
    }

    pub async fn get_positions_by_event_types_async(&self, root_path: &str, event_types: &[&str]) -> Result<alloc::vec::Vec<u64>, crate::ports::Error> {
        let mut all_positions = alloc::vec::Vec::new();
        for &et in event_types {
            let mut pos = self.event_type_index.get_positions_async(root_path, et).await?;
            all_positions.append(&mut pos);
        }
        all_positions.sort_unstable();
        all_positions.dedup();
        Ok(all_positions)
    }

    pub async fn get_positions_by_tag_async(&self, root_path: &str, tag: &crate::domain::Tag) -> Result<alloc::vec::Vec<u64>, crate::ports::Error> {
        self.tag_index.get_positions_async(root_path, tag).await
    }

    pub async fn get_positions_by_tags_async(&self, root_path: &str, tags: &[&crate::domain::Tag]) -> Result<alloc::vec::Vec<u64>, crate::ports::Error> {
        let mut all_positions = alloc::vec::Vec::new();
        for &t in tags {
            let mut pos = self.tag_index.get_positions_async(root_path, t).await?;
            all_positions.append(&mut pos);
        }
        all_positions.sort_unstable();
        all_positions.dedup();
        Ok(all_positions)
    }

    pub async fn event_type_index_exists(&self, root_path: &str, event_type: &str) -> bool {
        self.event_type_index.index_exists(root_path, event_type).await
    }

    pub async fn tag_index_exists(&self, root_path: &str, tag: &crate::domain::Tag) -> bool {
        self.tag_index.index_exists(root_path, tag).await
    }
}
"""

tests = """
    use crate::domain::EventRecord;

    fn create_test_event_record(position: u64, event_type: &str, tags: Vec<Tag>) -> EventRecord {
        EventRecord {
            position,
            event_id: alloc::format!("evt-{}", position),
            event: DomainEvent {
                event_type: event_type.to_string(),
                data: "{}".to_string(),
                tags,
            },
            metadata: None,
            timestamp: 123456789,
        }
    }

    #[tokio::test]
    async fn index_manager_add_event_adds_to_event_type_index() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = IndexManager::new(storage.clone());
        let evt = create_test_event_record(1, "TestEvent", vec![]);
        
        manager.add_event_to_indices_async("/temp", &evt).await.unwrap();
        
        assert!(manager.event_type_index_exists("/temp", "TestEvent").await);
        let pos = manager.get_positions_by_event_type_async("/temp", "TestEvent").await.unwrap();
        assert_eq!(pos, vec![1]);
    }

    #[tokio::test]
    async fn index_manager_add_event_adds_to_tag_indices() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = IndexManager::new(storage.clone());
        let tag1 = Tag { key: "Env".to_string(), value: "Prod".to_string() };
        let tag2 = Tag { key: "Region".to_string(), value: "US".to_string() };
        let evt = create_test_event_record(1, "TestEvent", vec![tag1.clone(), tag2.clone()]);
        
        manager.add_event_to_indices_async("/temp", &evt).await.unwrap();
        
        assert!(manager.tag_index_exists("/temp", &tag1).await);
        assert!(manager.tag_index_exists("/temp", &tag2).await);
        
        let p1 = manager.get_positions_by_tag_async("/temp", &tag1).await.unwrap();
        let p2 = manager.get_positions_by_tag_async("/temp", &tag2).await.unwrap();
        assert_eq!(p1, vec![1]);
        assert_eq!(p2, vec![1]);
    }

    #[tokio::test]
    async fn index_manager_get_by_event_types_returns_union() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = IndexManager::new(storage.clone());
        
        let evt1 = create_test_event_record(1, "TypeA", vec![]);
        let evt2 = create_test_event_record(2, "TypeB", vec![]);
        let evt3 = create_test_event_record(3, "TypeA", vec![]);
        
        manager.add_event_to_indices_async("/temp", &evt1).await.unwrap();
        manager.add_event_to_indices_async("/temp", &evt2).await.unwrap();
        manager.add_event_to_indices_async("/temp", &evt3).await.unwrap();
        
        let pos = manager.get_positions_by_event_types_async("/temp", &["TypeA", "TypeB"]).await.unwrap();
        assert_eq!(pos, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn index_manager_get_by_tags_returns_union() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = IndexManager::new(storage.clone());
        
        let tag1 = Tag { key: "Env".to_string(), value: "Prod".to_string() };
        let tag2 = Tag { key: "Region".to_string(), value: "US".to_string() };
        
        let evt1 = create_test_event_record(1, "TestEvent", vec![tag1.clone()]);
        let evt2 = create_test_event_record(2, "TestEvent", vec![tag2.clone()]);
        let evt3 = create_test_event_record(3, "TestEvent", vec![tag1.clone()]);
        
        manager.add_event_to_indices_async("/temp", &evt1).await.unwrap();
        manager.add_event_to_indices_async("/temp", &evt2).await.unwrap();
        manager.add_event_to_indices_async("/temp", &evt3).await.unwrap();
        
        let pos = manager.get_positions_by_tags_async("/temp", &[&tag1, &tag2]).await.unwrap();
        assert_eq!(pos, vec![1, 2, 3]);
    }
    
    #[tokio::test]
    async fn index_manager_integration_all_indices() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = IndexManager::new(storage.clone());
        
        let evt1 = create_test_event_record(1, "OrderCreated", vec![Tag { key: "Status".to_string(), value: "New".to_string() }]);
        let evt2 = create_test_event_record(2, "OrderPaid", vec![Tag { key: "Status".to_string(), value: "Paid".to_string() }]);
        
        manager.add_event_to_indices_async("/temp", &evt1).await.unwrap();
        manager.add_event_to_indices_async("/temp", &evt2).await.unwrap();
        
        let status_new = Tag { key: "Status".to_string(), value: "New".to_string() };
        assert!(manager.tag_index_exists("/temp", &status_new).await);
        
        let pos = manager.get_positions_by_event_type_async("/temp", "OrderPaid").await.unwrap();
        assert_eq!(pos, vec![2]);
    }
"""

patched = text.replace("#[cfg(test)]\nmod tests {", impls + "\n#[cfg(test)]\nmod tests {\n" + tests)
with open('src/event_store.rs', 'w') as f:
    f.write(patched)
