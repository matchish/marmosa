use crate::ports::StorageBackend;

#[derive(Debug, Clone)]
pub struct OpossumOptions {
    pub root_path: alloc::string::String,
    pub store_name: Option<alloc::string::String>,
}

impl OpossumOptions {
    pub fn new(root_path: impl Into<alloc::string::String>) -> Self {
        Self {
            root_path: root_path.into(),
            store_name: None,
        }
    }

    pub fn use_store(mut self, name: impl Into<alloc::string::String>) -> Self {
        self.store_name = Some(name.into());
        self
    }
}

pub struct StorageInitializer<S> {
    storage: S,
    options: OpossumOptions,
}

impl<S: crate::ports::StorageBackend> StorageInitializer<S> {
    pub fn new(storage: S, options: OpossumOptions) -> Self {
        Self { storage, options }
    }

    pub fn get_context_path(&self, context: &str) -> alloc::string::String {
        alloc::format!("{}/{}", self.options.root_path, context)
    }

    pub fn get_events_path(&self, context: &str) -> alloc::string::String {
        alloc::format!("{}/{}/Events", self.options.root_path, context)
    }

    pub fn get_ledger_path(&self, context: &str) -> alloc::string::String {
        alloc::format!("{}/{}/.ledger", self.options.root_path, context)
    }

    pub fn get_event_type_index_path(&self, context: &str) -> alloc::string::String {
        alloc::format!("{}/{}/Indices/EventType", self.options.root_path, context)
    }

    pub fn get_tags_index_path(&self, context: &str) -> alloc::string::String {
        alloc::format!("{}/{}/Indices/Tags", self.options.root_path, context)
    }

    pub async fn initialize(&self) -> Result<(), crate::ports::Error> {
        let store_name = self
            .options
            .store_name
            .as_ref()
            .ok_or(crate::ports::Error::IoError)?;

        let root = &self.options.root_path;
        let _ = self.storage.create_dir_all(root).await;

        let ctx = self.get_context_path(store_name);
        let _ = self.storage.create_dir_all(&ctx).await;

        let events = self.get_events_path(store_name);
        let _ = self.storage.create_dir_all(&events).await;

        let indices = alloc::format!("{}/{}/Indices", self.options.root_path, store_name);
        let _ = self.storage.create_dir_all(&indices).await;

        let event_type = self.get_event_type_index_path(store_name);
        let _ = self.storage.create_dir_all(&event_type).await;

        let tags = self.get_tags_index_path(store_name);
        let _ = self.storage.create_dir_all(&tags).await;

        let ledger = self.get_ledger_path(store_name);
        if let Err(crate::ports::Error::NotFound) = self.storage.read_file(&ledger).await {
            let _ = self.storage.write_file(&ledger, b"").await;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{DomainEvent, EventRecord, Tag};
    use crate::event_store::*;
    use crate::ports::tests::InMemoryStorage;

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
    async fn storage_initializer_initialize_with_no_store_name_fails() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let options = OpossumOptions::new("/test_root");
        let initializer = StorageInitializer::new(storage, options);

        let res = initializer.initialize().await;
        assert!(matches!(res, Err(crate::ports::Error::IoError)));
    }

    #[tokio::test]
    async fn storage_initializer_initialize_creates_structure() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let options = OpossumOptions::new("/test_root").use_store("CourseManagement");
        let initializer = StorageInitializer::new(storage.clone(), options);

        initializer.initialize().await.unwrap();

        // Check if directories exist by trying to list them or create them (they should just succeed)
        // Wait, InMemoryStorage read_dir works
        assert!(storage.read_dir("/test_root").await.is_ok());
        assert!(
            storage
                .read_dir("/test_root/CourseManagement")
                .await
                .is_ok()
        );
        assert!(
            storage
                .read_dir("/test_root/CourseManagement/Events")
                .await
                .is_ok()
        );
        assert!(
            storage
                .read_dir("/test_root/CourseManagement/Indices")
                .await
                .is_ok()
        );
        assert!(
            storage
                .read_dir("/test_root/CourseManagement/Indices/EventType")
                .await
                .is_ok()
        );
        assert!(
            storage
                .read_dir("/test_root/CourseManagement/Indices/Tags")
                .await
                .is_ok()
        );

        // Check if .ledger exists
        assert!(
            storage
                .read_file("/test_root/CourseManagement/.ledger")
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn storage_initializer_initialize_multiple_times() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let options = OpossumOptions::new("/test_root").use_store("CourseManagement");
        let initializer = StorageInitializer::new(storage.clone(), options);

        initializer.initialize().await.unwrap();
        initializer.initialize().await.unwrap(); // Should not fail
    }

    #[tokio::test]
    async fn storage_initializer_preserves_existing_ledger() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let options = OpossumOptions::new("/test_root").use_store("CourseManagement");
        let initializer = StorageInitializer::new(storage.clone(), options);

        storage
            .create_dir_all("/test_root/CourseManagement")
            .await
            .unwrap();
        storage
            .write_file("/test_root/CourseManagement/.ledger", b"existing content")
            .await
            .unwrap();

        initializer.initialize().await.unwrap();

        let content = storage
            .read_file("/test_root/CourseManagement/.ledger")
            .await
            .unwrap();
        assert_eq!(content, b"existing content");
    }
}
