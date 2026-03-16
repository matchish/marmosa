#[derive(Debug, Clone)]
pub struct OpossumOptions {
    pub root_path: alloc::string::String,
    pub store_name: Option<alloc::string::String>,
    pub flush_events_immediately: bool,
}

impl OpossumOptions {
    pub fn new(root_path: impl Into<alloc::string::String>) -> Self {
        Self {
            root_path: root_path.into(),
            store_name: None,
            flush_events_immediately: true,
        }
    }

    pub fn use_store(mut self, name: impl Into<alloc::string::String>) -> Self {
        if self.store_name.is_some() {
            panic!("UseStore has already been called");
        }
        let name_str = name.into();
        if name_str.trim().is_empty() {
            panic!("name cannot be empty");
        }
        if name_str.contains('/')
            || name_str.contains('\\')
            || name_str.contains(':')
            || name_str.contains('*')
            || name_str.contains('?')
        {
            panic!("Invalid store name");
        }
        self.store_name = Some(name_str);
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
    use crate::ports::StorageBackend;
    use super::*;
    use crate::ports::tests::InMemoryStorage;

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

    #[tokio::test]
    async fn initialize_creates_empty_ledger_file() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let options = OpossumOptions::new("/test_root").use_store("CourseManagement");
        let initializer = StorageInitializer::new(storage.clone(), options);

        initializer.initialize().await.unwrap();

        let content = storage.read_file("/test_root/CourseManagement/.ledger").await.unwrap();
        assert_eq!(content.len(), 0);
    }

    #[tokio::test]
    async fn initialize_with_relative_path_creates_directories() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let options = OpossumOptions::new("./test-data").use_store("CourseManagement");
        let initializer = StorageInitializer::new(storage.clone(), options);

        initializer.initialize().await.unwrap();

        assert!(storage.read_dir("./test-data").await.is_ok());
        assert!(storage.read_dir("./test-data/CourseManagement").await.is_ok());
    }

    #[tokio::test]
    async fn initialize_with_nested_path_creates_all_directories() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let options = OpossumOptions::new("/test_root/level1/level2/level3").use_store("CourseManagement");
        let initializer = StorageInitializer::new(storage.clone(), options);

        initializer.initialize().await.unwrap();

        assert!(storage.read_dir("/test_root/level1/level2/level3").await.is_ok());
        assert!(storage.read_dir("/test_root/level1/level2/level3/CourseManagement").await.is_ok());
    }

    #[test]
    fn constructor_sets_default_root_path() {
        let options = OpossumOptions::new("OpossumStore");
        assert_eq!(options.root_path, "OpossumStore");
    }

    #[test]
    fn constructor_store_name_is_null_by_default() {
        let options = OpossumOptions::new("OpossumStore");
        assert!(options.store_name.is_none());
    }

    #[test]
    fn use_store_with_valid_name_sets_store_name() {
        let options = OpossumOptions::new("root").use_store("CourseManagement");
        assert_eq!(options.store_name.unwrap(), "CourseManagement");
    }

    #[test]
    #[should_panic(expected = "UseStore has already been called")]
    fn use_store_when_called_twice_throws_invalid_operation_exception() {
        let _ = OpossumOptions::new("root")
            .use_store("CourseManagement")
            .use_store("Billing");
    }

    #[test]
    #[should_panic(expected = "name cannot be empty")]
    fn use_store_with_empty_name_throws_argument_exception() {
        let _ = OpossumOptions::new("root").use_store("");
    }

    #[test]
    #[should_panic(expected = "name cannot be empty")]
    fn use_store_with_whitespace_name_throws_argument_exception() {
        let _ = OpossumOptions::new("root").use_store("   ");
    }

    #[test]
    #[should_panic(expected = "Invalid store name")]
    fn use_store_with_invalid_characters_throws_argument_exception_slash() {
        let _ = OpossumOptions::new("root").use_store("Course/Management");
    }

    #[test]
    #[should_panic(expected = "Invalid store name")]
    fn use_store_with_invalid_characters_throws_argument_exception_backslash() {
        let _ = OpossumOptions::new("root").use_store("Course\\Management");
    }

    #[test]
    #[should_panic(expected = "Invalid store name")]
    fn use_store_with_invalid_characters_throws_argument_exception_colon() {
        let _ = OpossumOptions::new("root").use_store("Course:Management");
    }

    #[test]
    #[should_panic(expected = "Invalid store name")]
    fn use_store_with_invalid_characters_throws_argument_exception_star() {
        let _ = OpossumOptions::new("root").use_store("Course*Management");
    }

    #[test]
    #[should_panic(expected = "Invalid store name")]
    fn use_store_with_invalid_characters_throws_argument_exception_question() {
        let _ = OpossumOptions::new("root").use_store("Course?Management");
    }

    #[test]
    #[should_panic(expected = "UseStore has already been called")]
    fn use_store_when_called_twice_with_same_name_throws() {
        let _ = OpossumOptions::new("root")
            .use_store("CourseManagement")
            .use_store("CourseManagement");
    }

    #[test]
    fn root_path_can_be_set() {
        let mut options = OpossumOptions::new("");
        options.root_path = "/custom/path/to/store".to_string();
        assert_eq!(options.root_path, "/custom/path/to/store");
    }

    #[test]
    fn root_path_can_be_set_to_relative_path() {
        let mut options = OpossumOptions::new("");
        options.root_path = "./data/events".to_string();
        assert_eq!(options.root_path, "./data/events");
    }

    #[test]
    fn use_store_with_valid_names_sets_store_name_various() {
        let valid_names = [
            "ValidContext",
            "Context123",
            "Context_With_Underscores",
            "Context-With-Dashes",
            "Context.With.Dots",
            "CourseManagement",
        ];
        for name in valid_names {
            let options = OpossumOptions::new("root").use_store(name);
            assert_eq!(options.store_name.unwrap(), name);
        }
    }

    #[test]
    fn constructor_sets_flush_events_immediately_to_true_by_default() {
        let options = OpossumOptions::new("root");
        assert!(options.flush_events_immediately);
    }

    #[test]
    fn flush_events_immediately_can_be_set_to_false() {
        let mut options = OpossumOptions::new("root");
        options.flush_events_immediately = false;
        assert!(!options.flush_events_immediately);
    }

    #[test]
    fn flush_events_immediately_can_be_set_to_true() {
        let mut options = OpossumOptions::new("root");
        options.flush_events_immediately = false;
        options.flush_events_immediately = true;
        assert!(options.flush_events_immediately);
    }
}
