use alloc::format;
use alloc::vec::Vec;
use serde_json_core::to_vec;

use crate::domain::AppendCondition;
use crate::domain::EventData;
use crate::domain::EventRecord;
use crate::domain::Query;
use crate::ports::{Clock, Error, StorageBackend};

pub trait EventStore {
    fn append_async(
        &self,
        events: Vec<EventData>,
        condition: Option<AppendCondition>,
    ) -> impl core::future::Future<Output = Result<(), Error>> + Send;

    fn read_async(
        &self,
        query: Query,
        start_position: Option<u64>,
        max_count: Option<usize>,
    ) -> impl core::future::Future<Output = Result<Vec<EventRecord>, Error>> + Send;
}

pub struct OpossumStore<S, C> {
    storage: S,
    clock: C,
}

impl<S, C> OpossumStore<S, C> {
    pub fn new(storage: S, clock: C) -> Self {
        Self { storage, clock }
    }
}

impl<S: StorageBackend + Send + Sync, C: Clock + Send + Sync> OpossumStore<S, C> {
    async fn read_internal(
        &self,
        query: Query,
        start_position: Option<u64>,
        max_count: Option<usize>,
    ) -> Result<Vec<EventRecord>, Error> {
        let dir_path = "Events";
        let mut results = Vec::new();

        let sequence_files = self.storage.read_dir(dir_path).await.unwrap_or_default();
        let total_count = sequence_files.len() as u64;

        let start = start_position.map(|p| p + 1).unwrap_or(0); // Exclusive bound

        for current_pos in start..total_count {
            let file_path = format!("{}/{:010}.json", dir_path, current_pos);
            let data = self.storage.read_file(&file_path).await?;
            let (record, _) = serde_json_core::from_slice::<EventRecord>(&data).map_err(|_| Error::IoError)?;
            
            if query.matches(&record) {
                results.push(record);
                if let Some(max) = max_count {
                    if results.len() >= max {
                        break;
                    }
                }
            }
        }

        Ok(results)
    }
}

impl<S: StorageBackend + Send + Sync, C: Clock + Send + Sync> EventStore for OpossumStore<S, C> {
    async fn append_async(
        &self,
        events: Vec<EventData>,
        condition: Option<AppendCondition>,
    ) -> Result<(), Error> {
        let global_lock = "global_store";
        self.storage.acquire_stream_lock(global_lock).await?;

        let result = async {
            let dir_path = "Events";
            let _ = self.storage.create_dir_all(&dir_path).await; // Ignore if exists
            
            let existing_files = self.storage.read_dir(&dir_path).await.unwrap_or_default();
            let mut sequence = existing_files.len() as u64;

            if let Some(cond) = condition {
                // Read all current events to check against condition
                let current_events = self.read_internal(Query::all(), cond.after_sequence_position, None).await?;
                for evt in current_events {
                    if cond.fail_if_events_match.matches(&evt) {
                        return Err(Error::AppendConditionFailed);
                    }
                }
            }

            let timestamp = self.clock.now_millis();

            for event in events {
                let record = EventRecord {
                    position: sequence,
                    event_id: event.event_id,
                    event: event.event,
                    metadata: event.metadata,
                    timestamp,
                };

                let vec = to_vec::<_, 4096>(&record).map_err(|_| Error::IoError)?; // Map error appropriately

                let file_path = format!("{}/{:010}.json", dir_path, sequence);
                self.storage.write_file(&file_path, &vec).await?;
                
                sequence += 1;
            }

            Ok(())
        }.await;

        let _ = self.storage.release_stream_lock(global_lock).await;
        result
    }

    async fn read_async(
        &self,
        query: Query,
        start_position: Option<u64>,
        max_count: Option<usize>,
    ) -> Result<Vec<EventRecord>, Error> {
        self.read_internal(query, start_position, max_count).await
    }
}



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

    pub async fn add_position_async(&self, root_path: &str, tag: &crate::domain::Tag, position: u64) -> Result<(), crate::ports::Error> {
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

    pub async fn get_positions_async(&self, root_path: &str, tag: &crate::domain::Tag) -> Result<alloc::vec::Vec<u64>, crate::ports::Error> {
        let file_path = self.get_file_path(root_path, tag);
        match self.storage.read_file(&file_path).await {
            Ok(data) => {
                match serde_json_core::from_slice::<alloc::vec::Vec<u64>>(&data) {
                    Ok((pos, _)) => Ok(pos),
                    Err(_) => Ok(alloc::vec::Vec::new()),
                }
            }
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

    pub async fn add_position_async(&self, root_path: &str, event_type: &str, position: u64) -> Result<(), crate::ports::Error> {
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

    pub async fn get_positions_async(&self, root_path: &str, event_type: &str) -> Result<alloc::vec::Vec<u64>, crate::ports::Error> {
        let file_path = self.get_file_path(root_path, event_type);
        match self.storage.read_file(&file_path).await {
            Ok(data) => {
                match serde_json_core::from_slice::<alloc::vec::Vec<u64>>(&data) {
                    Ok((pos, _)) => Ok(pos),
                    Err(_) => Ok(alloc::vec::Vec::new()),
                }
            }
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



#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct LedgerData {
    pub last_sequence_position: u64,
    pub event_count: u64,
}

pub struct LedgerManager<S> {
    storage: S,
}

impl<S: crate::ports::StorageBackend + Send + Sync + Clone> LedgerManager<S> {
    pub fn new(storage: S) -> Self {
        Self { storage }
    }

    fn ledger_path(&self, context_path: &str) -> alloc::string::String {
        alloc::format!("{}/.ledger", context_path)
    }

    pub async fn get_last_sequence_position_async(&self, context_path: &str) -> Result<u64, crate::ports::Error> {
        let path = self.ledger_path(context_path);
        match self.storage.read_file(&path).await {
            Ok(data) => {
                if data.is_empty() {
                    return Ok(0);
                }
                match serde_json_core::from_slice::<LedgerData>(&data) {
                    Ok((ledger, _)) => Ok(ledger.last_sequence_position),
                    Err(_) => Err(crate::ports::Error::IoError), // Corrupt ledger
                }
            }
            Err(crate::ports::Error::NotFound) => Ok(0),
            Err(e) => Err(e),
        }
    }

    pub async fn get_next_sequence_position_async(&self, context_path: &str) -> Result<u64, crate::ports::Error> {
        let last = self.get_last_sequence_position_async(context_path).await?;
        if last == 0 && self.storage.read_file(&self.ledger_path(context_path)).await == Err(crate::ports::Error::NotFound) {
            Ok(1)
        } else {
            Ok(last + 1)
        }
    }

    pub async fn update_sequence_position_async(&self, context_path: &str, position: u64) -> Result<(), crate::ports::Error> {
        let _ = self.storage.create_dir_all(context_path).await;
        let path = self.ledger_path(context_path);
        let data = LedgerData {
            last_sequence_position: position,
            event_count: position,
        };
        let vec = serde_json_core::to_vec::<_, 1024>(&data).map_err(|_| crate::ports::Error::IoError)?;
        self.storage.write_file(&path, &vec).await?;
        Ok(())
    }

    pub async fn acquire_lock_async(&self, context_path: &str) -> Result<(), crate::ports::Error> {
        self.storage.acquire_stream_lock(context_path).await
    }

    pub async fn release_lock_async(&self, context_path: &str) -> Result<(), crate::ports::Error> {
        self.storage.release_stream_lock(context_path).await
    }
    
    pub async fn reconcile_ledger_async(&self, context_path: &str, events_dir: &str) -> Result<(), crate::ports::Error> {
        let files = match self.storage.read_dir(events_dir).await {
            Ok(f) => f,
            Err(_) => return Ok(()), // no dir means nothing to reconcile
        };
        
        let max_pos = files.iter().filter_map(|f| {
            f.split('/').last()?.strip_suffix(".json")?.parse::<u64>().ok()
        }).max().unwrap_or(0);
        
        let last = self.get_last_sequence_position_async(context_path).await?;
        if max_pos > last {
            self.update_sequence_position_async(context_path, max_pos).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {

    #[tokio::test]
    async fn ledger_get_last_sequence_position_when_no_exists_returns_zero() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = LedgerManager::new(storage.clone());
        let pos = manager.get_last_sequence_position_async("/temp").await.unwrap();
        assert_eq!(pos, 0);
    }

    #[tokio::test]
    async fn ledger_get_last_sequence_position_when_exists_returns_last() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = LedgerManager::new(storage.clone());
        manager.update_sequence_position_async("/temp", 42).await.unwrap();
        let pos = manager.get_last_sequence_position_async("/temp").await.unwrap();
        assert_eq!(pos, 42);
    }

    #[tokio::test]
    async fn ledger_get_last_sequence_position_when_corrupted_errors() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = LedgerManager::new(storage.clone());
        let _ = storage.create_dir_all("/temp").await;
        let _ = storage.write_file("/temp/.ledger", b"{ invalid JSON }").await;
        let p = manager.get_last_sequence_position_async("/temp").await;
        assert!(p.is_err());
    }
    
    #[tokio::test]
    async fn ledger_get_next_sequence_position_when_not_exists_returns_one() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = LedgerManager::new(storage.clone());
        let pos = manager.get_next_sequence_position_async("/temp").await.unwrap();
        assert_eq!(pos, 1);
    }

    #[tokio::test]
    async fn ledger_get_next_sequence_position_when_exists_returns_incremented() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = LedgerManager::new(storage.clone());
        manager.update_sequence_position_async("/temp", 42).await.unwrap();
        let pos = manager.get_next_sequence_position_async("/temp").await.unwrap();
        assert_eq!(pos, 43); // Because it was at 42, returning last + 1
    }

    #[tokio::test]
    async fn ledger_update_sequence_position_overwrites() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = LedgerManager::new(storage.clone());
        manager.update_sequence_position_async("/temp", 10).await.unwrap();
        manager.update_sequence_position_async("/temp", 20).await.unwrap();
        let pos = manager.get_last_sequence_position_async("/temp").await.unwrap();
        assert_eq!(pos, 20);
    }

    #[tokio::test]
    async fn ledger_acquire_lock_prevents_simultaneous() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = alloc::sync::Arc::new(LedgerManager::new(storage.clone()));
        
        manager.acquire_lock_async("/temp").await.unwrap();
        
        // Spawn a task that tries to acquire lock and fails using tokio::time::timeout
        let manager2 = manager.clone();
        let handle = tokio::spawn(async move {
            let res = tokio::time::timeout(core::time::Duration::from_millis(50), manager2.acquire_lock_async("/temp")).await;
            res.is_err() // Should timeout meaning it was blocked
        });
        
        let did_timeout = handle.await.unwrap();
        assert!(did_timeout, "Should have timed out waiting for lock");
        
        manager.release_lock_async("/temp").await.unwrap();
        assert!(manager.acquire_lock_async("/temp").await.is_ok());
    }

    #[tokio::test]
    async fn ledger_reconcile_when_events_exist_beyond_ledger() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = LedgerManager::new(storage.clone());
        
        manager.update_sequence_position_async("/temp", 3).await.unwrap();
        let _ = storage.create_dir_all("/temp/events").await;
        // Write event files directly
        let _ = storage.write_file("/temp/events/0000000004.json", b"{}").await;
        let _ = storage.write_file("/temp/events/0000000005.json", b"{}").await;
        
        manager.reconcile_ledger_async("/temp", "/temp/events").await.unwrap();
        
        let pos = manager.get_last_sequence_position_async("/temp").await.unwrap();
        assert_eq!(pos, 5);
    }

    #[tokio::test]
    async fn ledger_reconcile_noop_when_correct() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = LedgerManager::new(storage.clone());
        
        manager.update_sequence_position_async("/temp", 5).await.unwrap();
        let _ = storage.create_dir_all("/temp/events").await;
        let _ = storage.write_file("/temp/events/0000000003.json", b"{}").await;
        let _ = storage.write_file("/temp/events/0000000005.json", b"{}").await;
        
        manager.reconcile_ledger_async("/temp", "/temp/events").await.unwrap();
        
        let pos = manager.get_last_sequence_position_async("/temp").await.unwrap();
        assert_eq!(pos, 5);
    }

    #[tokio::test]
    async fn ledger_reconcile_handles_empty_events() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let manager = LedgerManager::new(storage.clone());
        
        manager.update_sequence_position_async("/temp", 3).await.unwrap();
        let _ = storage.create_dir_all("/temp/events").await;
        
        manager.reconcile_ledger_async("/temp", "/temp/events").await.unwrap();
        
        let pos = manager.get_last_sequence_position_async("/temp").await.unwrap();
        assert_eq!(pos, 3);
    }


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


    use crate::domain::Tag;

    #[tokio::test]
    async fn tag_index_add_position_creates_index_file() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let index = TagIndex::new(storage.clone());
        let tag = Tag { key: "Environment".to_string(), value: "Production".to_string() };
        
        index.add_position_async("/temp", &tag, 1).await.unwrap();
        assert!(index.index_exists("/temp", &tag).await);
    }

    #[tokio::test]
    async fn tag_index_add_multiple_positions_sorts_them() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let index = TagIndex::new(storage.clone());
        let tag = Tag { key: "Environment".to_string(), value: "Production".to_string() };
        
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
        let tag = Tag { key: "Environment".to_string(), value: "Production".to_string() };
        
        index.add_position_async("/temp", &tag, 1).await.unwrap();
        index.add_position_async("/temp", &tag, 1).await.unwrap();
        
        let positions = index.get_positions_async("/temp", &tag).await.unwrap();
        assert_eq!(positions, vec![1]);
    }

    #[tokio::test]
    async fn tag_index_special_characters_creates_safe_filename() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let index = TagIndex::new(storage.clone());
        let tag = Tag { key: "Key:With*Special?".to_string(), value: "Value/With".to_string() };
        
        index.add_position_async("/temp", &tag, 1).await.unwrap();
        assert!(index.index_exists("/temp", &tag).await);
    }

    #[tokio::test]
    async fn tag_index_after_corruption_rebuilds() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let index = TagIndex::new(storage.clone());
        let tag = Tag { key: "Env".to_string(), value: "Prod".to_string() };
        
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
        let tag = alloc::sync::Arc::new(Tag { key: "Env".to_string(), value: "Prod".to_string() });
        
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
        
        index.add_position_async("/temp", "TestEvent", 1).await.unwrap();
        assert!(index.index_exists("/temp", "TestEvent").await);
    }

    #[tokio::test]
    async fn event_type_index_add_multiple_positions_sorts_them() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let index = EventTypeIndex::new(storage.clone());
        
        index.add_position_async("/temp", "TestEvent", 5).await.unwrap();
        index.add_position_async("/temp", "TestEvent", 2).await.unwrap();
        index.add_position_async("/temp", "TestEvent", 7).await.unwrap();
        
        let positions = index.get_positions_async("/temp", "TestEvent").await.unwrap();
        assert_eq!(positions, vec![2, 5, 7]);
    }

    #[tokio::test]
    async fn event_type_index_ignores_duplicates() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let index = EventTypeIndex::new(storage.clone());
        
        index.add_position_async("/temp", "TestEvent", 1).await.unwrap();
        index.add_position_async("/temp", "TestEvent", 1).await.unwrap();
        
        let positions = index.get_positions_async("/temp", "TestEvent").await.unwrap();
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
                idx.add_position_async("/temp", "TestEvent", i).await.unwrap();
            }));
        }

        for handle in handles {
            let _ = handle.await;
        }

        let positions = index.get_positions_async("/temp", "TestEvent").await.unwrap();
        assert_eq!(positions.len(), 20);
        let expected: Vec<u64> = (1..=20).collect();
        assert_eq!(positions, expected);
    }

    use super::*;
    use alloc::string::ToString;
    use alloc::vec;
    use crate::ports::tests::{FakeClock, InMemoryStorage};
    use crate::domain::DomainEvent;

    #[tokio::test]
    async fn test_append_single_event_to_new_stream() {
        let storage = InMemoryStorage::new();
        let clock = FakeClock::new(1696000000);
        let store = OpossumStore::new(storage, clock);

        let event = EventData {
            event_id: "evt-123".to_string(),
            event: DomainEvent {
                event_type: "TestEvent".to_string(),
                data: "{\"foo\":\"bar\"}".to_string(),
                tags: vec![],
            },
            metadata: None,
        };

        let result = store.append_async(vec![event], None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_append_and_read_stream() {
        let storage = InMemoryStorage::new();
        let clock = FakeClock::new(1696000000);
        let store = OpossumStore::new(storage, clock);

        let event1 = EventData {
            event_id: "evt-1".to_string(),
            event: DomainEvent {
                event_type: "TestEvent".to_string(),
                data: "{}".to_string(),
                tags: vec![],
            },
            metadata: None,
        };
        store.append_async(vec![event1], None).await.unwrap();

        let events = store.read_async(Query::all(), None, Some(10)).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_id, "evt-1");
        assert_eq!(events[0].position, 0);
    }

    #[tokio::test]
    async fn test_append_condition_stream_does_not_exist() {
        let storage = InMemoryStorage::new();
        let clock = FakeClock::new(1696000000);
        let store = OpossumStore::new(storage, clock);

        let event = EventData {
            event_id: "evt-1".to_string(),
            event: DomainEvent {
                event_type: "TestEvent".to_string(),
                data: "{}".to_string(),
                tags: vec![],
            },
            metadata: None,
        };

        let cond = AppendCondition { fail_if_events_match: Query::all(), after_sequence_position: None };

        // First append should succeed
        let res = store.append_async(vec![event], Some(cond.clone())).await;
        assert!(res.is_ok());

        let event2 = EventData {
            event_id: "evt-2".to_string(),
            event: DomainEvent {
                event_type: "TestEvent".to_string(),
                data: "{}".to_string(),
                tags: vec![],
            },
            metadata: None,
        };

        // Second append should fail because events exist
        let res = store.append_async(vec![event2], Some(cond)).await;
        assert_eq!(res, Err(Error::AppendConditionFailed));
    }

    #[tokio::test]
    async fn test_append_condition_expected_position() {
        let storage = InMemoryStorage::new();
        let clock = FakeClock::new(1696000000);
        let store = OpossumStore::new(storage, clock);

        let event = EventData {
            event_id: "evt-1".to_string(),
            event: DomainEvent {
                event_type: "TestEvent".to_string(),
                data: "{}".to_string(),
                tags: vec![],
            },
            metadata: None,
        };

        // Append at position 0 (first event)
        let res = store.append_async(vec![event], None).await;
        assert!(res.is_ok());

        let event2 = EventData {
            event_id: "evt-2".to_string(),
            event: DomainEvent {
                event_type: "TestEvent".to_string(),
                data: "{}".to_string(),
                tags: vec![],
            },
            metadata: None,
        };

        let cond = AppendCondition { fail_if_events_match: Query::all(), after_sequence_position: Some(0) };
        let res = store.append_async(vec![event2], Some(cond)).await;
        assert!(res.is_ok());

        let event3 = EventData {
            event_id: "evt-3".to_string(),
            event: DomainEvent {
                event_type: "TestEvent".to_string(),
                data: "{}".to_string(),
                tags: vec![],
            },
            metadata: None,
        };

        let failing_cond = AppendCondition { fail_if_events_match: Query::all(), after_sequence_position: Some(0) };

        // Attempting to append expecting position 0 again should fail because there is an event at pos 1
        let res = store.append_async(vec![event3], Some(failing_cond)).await;
        assert_eq!(res, Err(Error::AppendConditionFailed));
    }
}
