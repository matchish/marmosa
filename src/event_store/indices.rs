pub struct TagIndex<S> {
    storage: S,
}

impl<S: crate::ports::StorageBackend + Send + Sync> TagIndex<S> {
    pub fn new(storage: S) -> Self {
        Self { storage }
    }

    fn sanitize_name(name: &str) -> alloc::string::String {
        name.replace("/", "_")
            .replace(":", "_")
            .replace("*", "_")
            .replace("?", "_")
            .to_lowercase()
    }

    fn get_file_path(&self, root_path: &str, tag: &crate::domain::Tag) -> alloc::string::String {
        let safe_key = Self::sanitize_name(&tag.key);
        let safe_value = Self::sanitize_name(&tag.value);
        alloc::format!("{}/Tags/{}_{}.json", root_path, safe_key, safe_value)
    }

    pub async fn add_position_async(
        &self,
        root_path: &str,
        tag: &crate::domain::Tag,
        position: u64,
    ) -> Result<(), crate::ports::Error> {
        let dir_path = alloc::format!("{}/Tags", root_path);
        let _ = self.storage.create_dir_all(&dir_path).await;

        let file_path = self.get_file_path(root_path, tag);
        let lock_key = alloc::format!("tag_lock:{}", file_path);

        self.storage.acquire_stream_lock(&lock_key).await?;

        let mut positions = match self.storage.read_file(&file_path).await {
            Ok(data) => {
                match serde_json_core::from_slice::<alloc::vec::Vec<u64>>(&data) {
                    Ok((pos, _)) => pos,
                    Err(_) => alloc::vec::Vec::new(), // corruption recovery
                }
            }
            Err(_) => alloc::vec::Vec::new(),
        };

        if !positions.contains(&position) {
            positions.push(position);
            positions.sort_unstable();
            if let Ok(data) = serde_json_core::to_vec::<_, 16384>(&positions) {
                let _ = self.storage.write_file(&file_path, &data).await;
            }
        }

        self.storage.release_stream_lock(&lock_key).await?;
        Ok(())
    }

    pub async fn get_positions_async(
        &self,
        root_path: &str,
        tag: &crate::domain::Tag,
    ) -> Result<alloc::vec::Vec<u64>, crate::ports::Error> {
        let file_path = self.get_file_path(root_path, tag);
        match self.storage.read_file(&file_path).await {
            Ok(data) => match serde_json_core::from_slice::<alloc::vec::Vec<u64>>(&data) {
                Ok((pos, _)) => Ok(pos),
                Err(_) => Ok(alloc::vec::Vec::new()),
            },
            Err(_) => Ok(alloc::vec::Vec::new()),
        }
    }

    pub async fn index_exists(&self, root_path: &str, tag: &crate::domain::Tag) -> bool {
        let file_path = self.get_file_path(root_path, tag);
        self.storage.read_file(&file_path).await.is_ok()
    }
}

pub struct EventTypeIndex<S> {
    storage: S,
}

impl<S: crate::ports::StorageBackend + Send + Sync> EventTypeIndex<S> {
    pub fn new(storage: S) -> Self {
        Self { storage }
    }

    fn sanitize_name(name: &str) -> alloc::string::String {
        name.replace("/", "_")
            .replace(":", "_")
            .replace("*", "_")
            .replace("?", "_")
            .to_lowercase()
    }

    fn get_file_path(&self, root_path: &str, event_type: &str) -> alloc::string::String {
        let safe_type = Self::sanitize_name(event_type);
        alloc::format!("{}/EventTypes/{}.json", root_path, safe_type)
    }

    pub async fn add_position_async(
        &self,
        root_path: &str,
        event_type: &str,
        position: u64,
    ) -> Result<(), crate::ports::Error> {
        let dir_path = alloc::format!("{}/EventTypes", root_path);
        let _ = self.storage.create_dir_all(&dir_path).await;

        let file_path = self.get_file_path(root_path, event_type);
        let lock_key = alloc::format!("event_type_lock:{}", file_path);

        self.storage.acquire_stream_lock(&lock_key).await?;

        let mut positions = match self.storage.read_file(&file_path).await {
            Ok(data) => {
                match serde_json_core::from_slice::<alloc::vec::Vec<u64>>(&data) {
                    Ok((pos, _)) => pos,
                    Err(_) => alloc::vec::Vec::new(), // corruption recovery
                }
            }
            Err(_) => alloc::vec::Vec::new(),
        };

        if !positions.contains(&position) {
            positions.push(position);
            positions.sort_unstable();
            if let Ok(data) = serde_json_core::to_vec::<_, 16384>(&positions) {
                let _ = self.storage.write_file(&file_path, &data).await;
            }
        }

        self.storage.release_stream_lock(&lock_key).await?;
        Ok(())
    }

    pub async fn get_positions_async(
        &self,
        root_path: &str,
        event_type: &str,
    ) -> Result<alloc::vec::Vec<u64>, crate::ports::Error> {
        let file_path = self.get_file_path(root_path, event_type);
        match self.storage.read_file(&file_path).await {
            Ok(data) => match serde_json_core::from_slice::<alloc::vec::Vec<u64>>(&data) {
                Ok((pos, _)) => Ok(pos),
                Err(_) => Ok(alloc::vec::Vec::new()),
            },
            Err(_) => Ok(alloc::vec::Vec::new()),
        }
    }

    pub async fn index_exists(&self, root_path: &str, event_type: &str) -> bool {
        let file_path = self.get_file_path(root_path, event_type);
        self.storage.read_file(&file_path).await.is_ok()
    }
}

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

    pub async fn add_event_to_indices_async(
        &self,
        root_path: &str,
        record: &crate::domain::EventRecord,
    ) -> Result<(), crate::ports::Error> {
        self.event_type_index
            .add_position_async(root_path, &record.event.event_type, record.position)
            .await?;
        for tag in &record.event.tags {
            self.tag_index
                .add_position_async(root_path, tag, record.position)
                .await?;
        }
        Ok(())
    }

    pub async fn get_positions_by_event_type_async(
        &self,
        root_path: &str,
        event_type: &str,
    ) -> Result<alloc::vec::Vec<u64>, crate::ports::Error> {
        self.event_type_index
            .get_positions_async(root_path, event_type)
            .await
    }

    pub async fn get_positions_by_event_types_async(
        &self,
        root_path: &str,
        event_types: &[&str],
    ) -> Result<alloc::vec::Vec<u64>, crate::ports::Error> {
        let mut all_positions = alloc::vec::Vec::new();
        for &et in event_types {
            let mut pos = self
                .event_type_index
                .get_positions_async(root_path, et)
                .await?;
            all_positions.append(&mut pos);
        }
        all_positions.sort_unstable();
        all_positions.dedup();
        Ok(all_positions)
    }

    pub async fn get_positions_by_tag_async(
        &self,
        root_path: &str,
        tag: &crate::domain::Tag,
    ) -> Result<alloc::vec::Vec<u64>, crate::ports::Error> {
        self.tag_index.get_positions_async(root_path, tag).await
    }

    pub async fn get_positions_by_tags_async(
        &self,
        root_path: &str,
        tags: &[&crate::domain::Tag],
    ) -> Result<alloc::vec::Vec<u64>, crate::ports::Error> {
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
        self.event_type_index
            .index_exists(root_path, event_type)
            .await
    }

    pub async fn tag_index_exists(&self, root_path: &str, tag: &crate::domain::Tag) -> bool {
        self.tag_index.index_exists(root_path, tag).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{DomainEvent, EventRecord, Tag};
    use crate::event_store::*;
    use crate::ports::tests::InMemoryStorage;

    #[allow(dead_code)]
    fn create_test_event(
        position: u64,
        event_type: &str,
        data: &str,
    ) -> crate::domain::EventRecord {
        crate::domain::EventRecord {
            position,
            event_id: alloc::string::String::from("id"),
            event: crate::domain::DomainEvent {
                event_type: alloc::string::String::from(event_type),
                data: alloc::string::String::from(data),
                tags: alloc::vec::Vec::new(),
            },
            metadata: None,
            timestamp: 0,
        }
    }

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
    async fn storage_initializer_get_paths() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let options = OpossumOptions::new("/test_root").use_store("CourseManagement");
        let initializer = StorageInitializer::new(storage, options);

        assert_eq!(
            initializer.get_context_path("CourseManagement"),
            "/test_root/CourseManagement"
        );
        assert_eq!(
            initializer.get_events_path("CourseManagement"),
            "/test_root/CourseManagement/Events"
        );
        assert_eq!(
            initializer.get_ledger_path("CourseManagement"),
            "/test_root/CourseManagement/.ledger"
        );
        assert_eq!(
            initializer.get_event_type_index_path("CourseManagement"),
            "/test_root/CourseManagement/Indices/EventType"
        );
        assert_eq!(
            initializer.get_tags_index_path("CourseManagement"),
            "/test_root/CourseManagement/Indices/Tags"
        );
    }

    #[tokio::test]
    async fn index_manager_add_event_adds_to_event_type_index() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = IndexManager::new(storage.clone());
        let evt = create_test_event_record(1, "TestEvent", vec![]);

        manager
            .add_event_to_indices_async("/temp", &evt)
            .await
            .unwrap();

        assert!(manager.event_type_index_exists("/temp", "TestEvent").await);
        let pos = manager
            .get_positions_by_event_type_async("/temp", "TestEvent")
            .await
            .unwrap();
        assert_eq!(pos, vec![1]);
    }

    #[tokio::test]
    async fn index_manager_add_event_adds_to_tag_indices() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = IndexManager::new(storage.clone());
        let tag1 = Tag {
            key: "Env".to_string(),
            value: "Prod".to_string(),
        };
        let tag2 = Tag {
            key: "Region".to_string(),
            value: "US".to_string(),
        };
        let evt = create_test_event_record(1, "TestEvent", vec![tag1.clone(), tag2.clone()]);

        manager
            .add_event_to_indices_async("/temp", &evt)
            .await
            .unwrap();

        assert!(manager.tag_index_exists("/temp", &tag1).await);
        assert!(manager.tag_index_exists("/temp", &tag2).await);

        let p1 = manager
            .get_positions_by_tag_async("/temp", &tag1)
            .await
            .unwrap();
        let p2 = manager
            .get_positions_by_tag_async("/temp", &tag2)
            .await
            .unwrap();
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

        manager
            .add_event_to_indices_async("/temp", &evt1)
            .await
            .unwrap();
        manager
            .add_event_to_indices_async("/temp", &evt2)
            .await
            .unwrap();
        manager
            .add_event_to_indices_async("/temp", &evt3)
            .await
            .unwrap();

        let pos = manager
            .get_positions_by_event_types_async("/temp", &["TypeA", "TypeB"])
            .await
            .unwrap();
        assert_eq!(pos, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn index_manager_get_by_tags_returns_union() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = IndexManager::new(storage.clone());

        let tag1 = Tag {
            key: "Env".to_string(),
            value: "Prod".to_string(),
        };
        let tag2 = Tag {
            key: "Region".to_string(),
            value: "US".to_string(),
        };

        let evt1 = create_test_event_record(1, "TestEvent", vec![tag1.clone()]);
        let evt2 = create_test_event_record(2, "TestEvent", vec![tag2.clone()]);
        let evt3 = create_test_event_record(3, "TestEvent", vec![tag1.clone()]);

        manager
            .add_event_to_indices_async("/temp", &evt1)
            .await
            .unwrap();
        manager
            .add_event_to_indices_async("/temp", &evt2)
            .await
            .unwrap();
        manager
            .add_event_to_indices_async("/temp", &evt3)
            .await
            .unwrap();

        let pos = manager
            .get_positions_by_tags_async("/temp", &[&tag1, &tag2])
            .await
            .unwrap();
        assert_eq!(pos, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn index_manager_integration_all_indices() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = IndexManager::new(storage.clone());

        let evt1 = create_test_event_record(
            1,
            "OrderCreated",
            vec![Tag {
                key: "Status".to_string(),
                value: "New".to_string(),
            }],
        );
        let evt2 = create_test_event_record(
            2,
            "OrderPaid",
            vec![Tag {
                key: "Status".to_string(),
                value: "Paid".to_string(),
            }],
        );

        manager
            .add_event_to_indices_async("/temp", &evt1)
            .await
            .unwrap();
        manager
            .add_event_to_indices_async("/temp", &evt2)
            .await
            .unwrap();

        let status_new = Tag {
            key: "Status".to_string(),
            value: "New".to_string(),
        };
        assert!(manager.tag_index_exists("/temp", &status_new).await);

        let pos = manager
            .get_positions_by_event_type_async("/temp", "OrderPaid")
            .await
            .unwrap();
        assert_eq!(pos, vec![2]);
    }

    #[tokio::test]
    async fn tag_index_add_position_creates_index_file() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let index = TagIndex::new(storage.clone());
        let tag = Tag {
            key: "Environment".to_string(),
            value: "Production".to_string(),
        };

        index.add_position_async("/temp", &tag, 1).await.unwrap();
        assert!(index.index_exists("/temp", &tag).await);
    }

    #[tokio::test]
    async fn tag_index_add_multiple_positions_sorts_them() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let index = TagIndex::new(storage.clone());
        let tag = Tag {
            key: "Environment".to_string(),
            value: "Production".to_string(),
        };

        index.add_position_async("/temp", &tag, 5).await.unwrap();
        index.add_position_async("/temp", &tag, 2).await.unwrap();
        index.add_position_async("/temp", &tag, 7).await.unwrap();

        let positions = index.get_positions_async("/temp", &tag).await.unwrap();
        assert_eq!(positions, vec![2, 5, 7]);
    }

    #[tokio::test]
    async fn tag_index_add_ignores_duplicates() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let index = TagIndex::new(storage.clone());
        let tag = Tag {
            key: "Environment".to_string(),
            value: "Production".to_string(),
        };

        index.add_position_async("/temp", &tag, 1).await.unwrap();
        index.add_position_async("/temp", &tag, 1).await.unwrap();

        let positions = index.get_positions_async("/temp", &tag).await.unwrap();
        assert_eq!(positions, vec![1]);
    }

    #[tokio::test]
    async fn tag_index_special_characters_creates_safe_filename() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let index = TagIndex::new(storage.clone());
        let tag = Tag {
            key: "Key:With*Special?".to_string(),
            value: "Value/With".to_string(),
        };

        index.add_position_async("/temp", &tag, 1).await.unwrap();
        assert!(index.index_exists("/temp", &tag).await);
    }

    #[tokio::test]
    async fn tag_index_after_corruption_rebuilds() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let index = TagIndex::new(storage.clone());
        let tag = Tag {
            key: "Env".to_string(),
            value: "Prod".to_string(),
        };

        let path = alloc::format!("/temp/Tags/{}_{}.json", "env", "prod");
        let _ = storage.create_dir_all("/temp/Tags").await;
        let _ = storage.write_file(&path, b"{ invalid json }").await;

        let positions = index.get_positions_async("/temp", &tag).await.unwrap();
        assert!(positions.is_empty());

        index.add_position_async("/temp", &tag, 5).await.unwrap();
        let positions = index.get_positions_async("/temp", &tag).await.unwrap();
        assert_eq!(positions, vec![5]);
    }

    #[tokio::test]
    async fn tag_index_concurrent_access() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let index = alloc::sync::Arc::new(TagIndex::new(storage.clone()));
        let tag = alloc::sync::Arc::new(Tag {
            key: "Env".to_string(),
            value: "Prod".to_string(),
        });

        let mut handles = vec![];
        for i in 1..=20 {
            let idx = index.clone();
            let t = tag.clone();
            handles.push(tokio::spawn(async move {
                idx.add_position_async("/temp", &t, i).await.unwrap();
            }));
        }

        for handle in handles {
            let _ = handle.await;
        }

        let positions = index.get_positions_async("/temp", &tag).await.unwrap();
        assert_eq!(positions.len(), 20);
        let expected: Vec<u64> = (1..=20).collect();
        assert_eq!(positions, expected);
    }

    #[tokio::test]
    async fn event_type_index_add_position_creates_index_file() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let index = EventTypeIndex::new(storage.clone());

        index
            .add_position_async("/temp", "TestEvent", 1)
            .await
            .unwrap();
        assert!(index.index_exists("/temp", "TestEvent").await);
    }

    #[tokio::test]
    async fn event_type_index_add_multiple_positions_sorts_them() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let index = EventTypeIndex::new(storage.clone());

        index
            .add_position_async("/temp", "TestEvent", 5)
            .await
            .unwrap();
        index
            .add_position_async("/temp", "TestEvent", 2)
            .await
            .unwrap();
        index
            .add_position_async("/temp", "TestEvent", 7)
            .await
            .unwrap();

        let positions = index
            .get_positions_async("/temp", "TestEvent")
            .await
            .unwrap();
        assert_eq!(positions, vec![2, 5, 7]);
    }

    #[tokio::test]
    async fn event_type_index_ignores_duplicates() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let index = EventTypeIndex::new(storage.clone());

        index
            .add_position_async("/temp", "TestEvent", 1)
            .await
            .unwrap();
        index
            .add_position_async("/temp", "TestEvent", 1)
            .await
            .unwrap();

        let positions = index
            .get_positions_async("/temp", "TestEvent")
            .await
            .unwrap();
        assert_eq!(positions, vec![1]);
    }

    #[tokio::test]
    async fn event_type_index_concurrent_access() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let index = alloc::sync::Arc::new(EventTypeIndex::new(storage.clone()));

        let mut handles = vec![];
        for i in 1..=20 {
            let idx = index.clone();
            handles.push(tokio::spawn(async move {
                idx.add_position_async("/temp", "TestEvent", i)
                    .await
                    .unwrap();
            }));
        }

        for handle in handles {
            let _ = handle.await;
        }

        let positions = index
            .get_positions_async("/temp", "TestEvent")
            .await
            .unwrap();
        assert_eq!(positions.len(), 20);
        let expected: Vec<u64> = (1..=20).collect();
        assert_eq!(positions, expected);
    }
}
