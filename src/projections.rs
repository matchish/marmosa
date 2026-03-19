//! Projection infrastructure and tag-based indexing helpers.
//!
//! # Overview
//!
//! Projections turn event streams into queryable state. When a projection store is configured
//! with a [`ProjectionTagProvider`], each saved projection state can also be indexed by tags.
//! This enables efficient lookups with [`ProjectionStore::query_by_tag`] and
//! [`ProjectionStore::query_by_tags`] instead of loading all projection states and filtering in
//! memory.
//!
//! # Examples
//!
//! ```rust
//! use marmosa::domain::Tag;
//! use marmosa::projections::ProjectionTagProvider;
//!
//! #[derive(Clone)]
//! struct StudentState {
//!     tier: String,
//!     is_maxed_out: bool,
//! }
//!
//! struct StudentTagProvider;
//!
//! impl ProjectionTagProvider<StudentState> for StudentTagProvider {
//!     fn get_tags(&self, state: &StudentState) -> Vec<Tag> {
//!         vec![
//!             Tag {
//!                 key: "Tier".to_string(),
//!                 value: state.tier.clone(),
//!             },
//!             Tag {
//!                 key: "IsMaxedOut".to_string(),
//!                 value: state.is_maxed_out.to_string(),
//!             },
//!         ]
//!     }
//! }
//!
//! let provider = StudentTagProvider;
//! let tags = provider.get_tags(&StudentState {
//!     tier: "Premium".to_string(),
//!     is_maxed_out: true,
//! });
//!
//! assert!(tags.iter().any(|t| t.key == "Tier" && t.value == "Premium"));
//! assert!(tags.iter().any(|t| t.key == "IsMaxedOut" && t.value == "true"));
//! ```
//!
//! # Notes
//!
//! - Tag keys and values are normalized to lowercase in the default storage-backed projection
//!   store before writing index files.
//! - Multi-tag queries use set intersection semantics (`AND`).

pub mod options;
pub mod related_events;
pub use options::*;
pub use related_events::*;

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use crate::domain::{EventRecord, Query};
use crate::ports::{Error, StorageBackend};

/// Defines how events are projected into a materialized view state
pub trait ProjectionDefinition {
    /// The projection state type
    type State;

    /// Unique name identifying this projection
    fn projection_name(&self) -> &str;

    /// Query filter for events this projection cares about
    fn event_types(&self) -> Query;

    /// Extracts the key from an event to identify which projection instance to update
    fn key_selector(&self, event: &EventRecord) -> Option<String>;

    /// Applies an event to the current state, returning the new state
    fn apply(&self, state: Option<Self::State>, event: &EventRecord) -> Option<Self::State>;

    /// Optional downcast to ProjectionWithRelatedEvents
    fn as_related_events(
        &self,
    ) -> Option<
        &dyn crate::projections::related_events::ProjectionWithRelatedEvents<State = Self::State>,
    > {
        None
    }
}

/// Storage interface for projection state - can be implemented for different backends
pub trait ProjectionTagProvider<TState> {
    /// Returns the tags that should be indexed for a projection state.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use marmosa::domain::Tag;
    /// use marmosa::projections::ProjectionTagProvider;
    ///
    /// #[derive(Clone)]
    /// struct State {
    ///     status: String,
    /// }
    ///
    /// struct Provider;
    ///
    /// impl ProjectionTagProvider<State> for Provider {
    ///     fn get_tags(&self, state: &State) -> Vec<Tag> {
    ///         vec![Tag {
    ///             key: "Status".to_string(),
    ///             value: state.status.clone(),
    ///         }]
    ///     }
    /// }
    ///
    /// let provider = Provider;
    /// let tags = provider.get_tags(&State {
    ///     status: "Active".to_string(),
    /// });
    ///
    /// assert_eq!(tags.len(), 1);
    /// assert_eq!(tags[0].key, "Status");
    /// assert_eq!(tags[0].value, "Active");
    /// ```
    fn get_tags(&self, state: &TState) -> Vec<crate::domain::Tag>;
}

pub struct NoopProjectionTagProvider;

#[derive(Clone, Default)]
pub struct NoopClock;
impl crate::ports::Clock for NoopClock {
    fn now_millis(&self) -> u64 {
        0
    }
}

impl<TState> ProjectionTagProvider<TState> for NoopProjectionTagProvider {
    fn get_tags(&self, _state: &TState) -> Vec<crate::domain::Tag> {
        Vec::new()
    }
}

pub trait ProjectionStore<TState> {
    /// Returns all projection states matching a single tag.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use marmosa::domain::Tag;
    /// use marmosa::ports::Error;
    /// use marmosa::projections::ProjectionStore;
    ///
    /// async fn load_premium<S, T>(store: &S) -> Result<Vec<T>, Error>
    /// where
    ///     S: ProjectionStore<T>,
    /// {
    ///     store
    ///         .query_by_tag(&Tag {
    ///             key: "Tier".to_string(),
    ///             value: "Premium".to_string(),
    ///         })
    ///         .await
    /// }
    /// ```
    fn query_by_tag(
        &self,
        tag: &crate::domain::Tag,
    ) -> impl core::future::Future<Output = Result<Vec<TState>, Error>> + Send;

    /// Returns all projection states that match every provided tag (`AND` semantics).
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use marmosa::domain::Tag;
    /// use marmosa::ports::Error;
    /// use marmosa::projections::ProjectionStore;
    ///
    /// async fn load_premium_and_maxed_out<S, T>(store: &S) -> Result<Vec<T>, Error>
    /// where
    ///     S: ProjectionStore<T>,
    /// {
    ///     let tags = vec![
    ///         Tag {
    ///             key: "Tier".to_string(),
    ///             value: "Premium".to_string(),
    ///         },
    ///         Tag {
    ///             key: "IsMaxedOut".to_string(),
    ///             value: "true".to_string(),
    ///         },
    ///     ];
    ///
    ///     store.query_by_tags(&tags).await
    /// }
    /// ```
    fn query_by_tags(
        &self,
        tags: &[crate::domain::Tag],
    ) -> impl core::future::Future<Output = Result<Vec<TState>, Error>> + Send;
    fn get(
        &self,
        key: &str,
    ) -> impl core::future::Future<Output = Result<Option<TState>, Error>> + Send;

    fn get_all(&self) -> impl core::future::Future<Output = Result<Vec<TState>, Error>> + Send;

    fn save(
        &self,
        key: &str,
        state: &TState,
    ) -> impl core::future::Future<Output = Result<(), Error>> + Send;

    fn delete(&self, key: &str) -> impl core::future::Future<Output = Result<(), Error>> + Send;
    fn clear(&self) -> impl core::future::Future<Output = Result<(), Error>> + Send;
}

/// Checkpoint for tracking projection progress
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ProjectionCheckpoint {
    pub projection_name: String,
    pub last_position: u64,
    pub last_updated: u64,
    pub total_events_processed: u64,
}

/// Runs a projection by reading events and applying them to the store
pub struct ProjectionRunner<S, TState, P, Store>
where
    P: ProjectionDefinition<State = TState>,
    Store: ProjectionStore<TState>,
{
    storage: S,
    projection: P,
    store: Store,
    _marker: core::marker::PhantomData<TState>,
}

impl<S, TState, P, Store> ProjectionRunner<S, TState, P, Store>
where
    S: StorageBackend + Send + Sync,
    TState: Serialize + for<'de> Deserialize<'de> + Send + Sync,
    P: ProjectionDefinition<State = TState> + Send + Sync,
    Store: ProjectionStore<TState> + Send + Sync,
{
    pub fn new(storage: S, projection: P, store: Store) -> Self {
        Self {
            storage,
            projection,
            store,
            _marker: core::marker::PhantomData,
        }
    }

    fn checkpoint_path(&self) -> String {
        alloc::format!(
            "Projections/_checkpoints/{}.json",
            self.projection.projection_name()
        )
    }

    pub async fn get_checkpoint(&self) -> Result<Option<ProjectionCheckpoint>, Error> {
        let path = self.checkpoint_path();
        match self.storage.read_file(&path).await {
            Ok(data) => {
                let checkpoint = serde_json::from_slice::<ProjectionCheckpoint>(&data)
                    .map_err(|_| Error::IoError)?;
                Ok(Some(checkpoint))
            }
            Err(Error::NotFound) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub async fn save_checkpoint(&self, position: u64, total_events: u64) -> Result<(), Error> {
        let checkpoint = ProjectionCheckpoint {
            projection_name: alloc::string::String::from(self.projection.projection_name()),
            last_position: position,
            last_updated: 0, // In a real implementation we would inject a clock here
            total_events_processed: total_events,
        };

        let _ = self
            .storage
            .create_dir_all("Projections/_checkpoints")
            .await;
        let path = self.checkpoint_path();
        let data = serde_json::to_vec(&checkpoint).map_err(|_| Error::IoError)?;
        self.storage.write_file(&path, &data).await
    }

    /// Process a batch of events starting from the last checkpoint

    pub async fn rebuild<E: crate::event_store::EventStore>(
        &self,
        event_store: &E,
    ) -> Result<u64, Error> {
        let lock_id = alloc::format!(".proj_lock_{}", self.projection.projection_name());
        self.storage.acquire_stream_lock(&lock_id).await?;

        let res = async {
            self.store.clear().await?;
            self.save_checkpoint(0, 0).await?;

            let events = event_store
                .read_async(crate::domain::Query::all(), None, None, None)
                .await?;
            self.process_events_with_store(&events, Some(event_store))
                .await
        }.await;

        let _ = self.storage.release_stream_lock(&lock_id).await;
        res
    }

    pub async fn process_events(&self, events: &[EventRecord]) -> Result<u64, Error> {
        self.process_events_internal(
            events,
            None::<&crate::event_store::MarmosaStore<S, crate::projections::NoopClock>>,
        )
        .await
    }

    pub async fn process_events_with_store<E: crate::event_store::EventStore>(
        &self,
        events: &[EventRecord],
        event_store: Option<&E>,
    ) -> Result<u64, Error> {
        self.process_events_internal(events, event_store).await
    }

    async fn process_events_internal<E: crate::event_store::EventStore>(
        &self,
        events: &[EventRecord],
        event_store: Option<&E>,
    ) -> Result<u64, Error> {
        let query = self.projection.event_types();
        let mut checkpoint = self
            .get_checkpoint()
            .await?
            .unwrap_or_else(|| ProjectionCheckpoint {
                projection_name: alloc::string::String::from(self.projection.projection_name()),
                last_position: 0,
                last_updated: 0,
                total_events_processed: 0,
            });

        for event in events {
            // Skip events we've already processed
            if event.position <= checkpoint.last_position && checkpoint.last_position > 0 {
                continue;
            }

            // Check if this event matches our query
            if !query.matches(event) {
                checkpoint.last_position = event.position;
                continue;
            }

            // Get the key for this event
            if let Some(key) = self.projection.key_selector(event) {
                // Load current state
                let current_state = self.store.get(&key).await?;

                // Apply event
                let new_state = if let Some(as_related) = self.projection.as_related_events() {
                    if let Some(query) = as_related.get_related_events_query(event) {
                        if let Some(store) = event_store {
                            let rel_events = store.read_async(query, None, None, None).await?;
                            as_related.apply_with_related(current_state, event, &rel_events)
                        } else {
                            // If we need related events but don't have a store, we could either error or pass empty.
                            // In a real framework we might error here.
                            as_related.apply_with_related(current_state, event, &[])
                        }
                    } else {
                        as_related.apply_with_related(current_state, event, &[])
                    }
                } else {
                    self.projection.apply(current_state, event)
                };
                match new_state {
                    Some(s) => self.store.save(&key, &s).await?,
                    None => self.store.delete(&key).await?,
                }
            }

            checkpoint.last_position = event.position;
            checkpoint.total_events_processed += 1;
        }

        // Save checkpoint
        if checkpoint.last_position > 0 {
            self.save_checkpoint(checkpoint.last_position, checkpoint.total_events_processed)
                .await?;
        }

        Ok(checkpoint.last_position)
    }
}

/// Default projection store backed by StorageBackend (file system, KV, etc.)
pub struct StorageBackendProjectionStore<
    S,
    TState,
    TTagProvider = NoopProjectionTagProvider,
    C = NoopClock,
> {
    storage: S,
    projection_name: alloc::string::String,
    tag_provider: Option<TTagProvider>,
    clock: Option<C>,
    _marker: core::marker::PhantomData<TState>,
}

impl<S, TState, P, C> StorageBackendProjectionStore<S, TState, P, C> {}

impl<S, TState, P, C> StorageBackendProjectionStore<S, TState, P, C> {
    fn get_projection_path(&self) -> String {
        alloc::format!("Projections/{}", self.projection_name)
    }

    fn get_file_path(&self, key: &str) -> String {
        alloc::format!("Projections/{}/{}.json", self.projection_name, key)
    }
}

impl<S, TState> StorageBackendProjectionStore<S, TState, NoopProjectionTagProvider, NoopClock> {
    pub fn new_with_clock(
        storage: S,
        projection_name: alloc::string::String,
        clock: Option<NoopClock>,
    ) -> Self {
        Self {
            storage,
            projection_name,
            tag_provider: None,
            clock,
            _marker: core::marker::PhantomData,
        }
    }

    pub fn new(storage: S, projection_name: alloc::string::String) -> Self {
        Self {
            storage,
            projection_name,
            tag_provider: None,
            clock: None,
            _marker: core::marker::PhantomData,
        }
    }

    pub fn new_with_tag_provider<P, C>(
        storage: S,
        projection_name: alloc::string::String,
        tag_provider: P,
    ) -> StorageBackendProjectionStore<S, TState, P, C> {
        StorageBackendProjectionStore {
            storage,
            projection_name,
            tag_provider: Some(tag_provider),
            clock: None,
            _marker: core::marker::PhantomData,
        }
    }

    pub fn new_with_tag_provider_and_clock<P, C>(
        storage: S,
        projection_name: alloc::string::String,
        tag_provider: P,
        clock: Option<C>,
    ) -> StorageBackendProjectionStore<S, TState, P, C> {
        StorageBackendProjectionStore {
            storage,
            projection_name,
            tag_provider: Some(tag_provider),
            clock,
            _marker: core::marker::PhantomData,
        }
    }
}

impl<
    S: StorageBackend + Send + Sync,
    TState: Serialize + for<'de> Deserialize<'de> + Send + Sync + Clone,
    TTagProvider: ProjectionTagProvider<TState> + Send + Sync,
    C: crate::ports::Clock + Send + Sync,
> ProjectionStore<TState> for StorageBackendProjectionStore<S, TState, TTagProvider, C>
{
    async fn query_by_tag(&self, tag: &crate::domain::Tag) -> Result<Vec<TState>, Error> {
        let dir_path = alloc::format!(
            "{}/Indices/{}/{}",
            self.get_projection_path(),
            tag.key.to_lowercase(),
            tag.value.to_lowercase()
        );
        let mut results = Vec::new();
        if let Ok(files) = self.storage.read_dir(&dir_path).await {
            let mut keys = Vec::new();
            for file in files {
                if file.ends_with(".json") {
                    let name = if let Some(pos) = file.rfind('/') {
                        &file[pos + 1..file.len() - 5]
                    } else {
                        &file[..file.len() - 5]
                    };
                    keys.push(alloc::string::ToString::to_string(name));
                }
            }
            for key in keys {
                if let Some(state) = self.get(&key).await? {
                    results.push(state);
                }
            }
        }
        Ok(results)
    }

    async fn query_by_tags(&self, tags: &[crate::domain::Tag]) -> Result<Vec<TState>, Error> {
        if tags.is_empty() {
            return Ok(Vec::new());
        }

        let mut candidate_keys: Option<alloc::collections::BTreeSet<alloc::string::String>> = None;

        for tag in tags {
            let dir_path = alloc::format!(
                "{}/Indices/{}/{}",
                self.get_projection_path(),
                tag.key.to_lowercase(),
                tag.value.to_lowercase()
            );
            let mut keys = alloc::collections::BTreeSet::new();
            if let Ok(files) = self.storage.read_dir(&dir_path).await {
                for file in files {
                    if file.ends_with(".json") {
                        let name = if let Some(pos) = file.rfind('/') {
                            &file[pos + 1..file.len() - 5]
                        } else {
                            &file[..file.len() - 5]
                        };
                        keys.insert(alloc::string::ToString::to_string(name));
                    }
                }
            }
            if let Some(mut existing) = candidate_keys {
                existing.retain(|k| keys.contains(k));
                candidate_keys = Some(existing);
            } else {
                candidate_keys = Some(keys);
            }
            if let Some(existing) = &candidate_keys
                && existing.is_empty()
            {
                return Ok(Vec::new());
            }
        }

        let mut results = Vec::new();
        if let Some(keys) = candidate_keys {
            for key in keys {
                if let Some(state) = self.get(&key).await? {
                    results.push(state);
                }
            }
        }
        Ok(results)
    }
    async fn clear(&self) -> Result<(), Error> {
        let dir_path = alloc::format!("Projections/{}", self.projection_name);
        let _ = self.storage.delete_dir_all(&dir_path).await;
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Option<TState>, Error> {
        let path = self.get_file_path(key);
        match self.storage.read_file(&path).await {
            Ok(data) => {
                if let Ok(wrapper) = serde_json::from_slice::<ProjectionWrapper<TState>>(&data) {
                    Ok(Some(wrapper.data))
                } else if let Ok(state) = serde_json::from_slice::<TState>(&data) {
                    Ok(Some(state))
                } else {
                    Err(Error::IoError)
                }
            }
            Err(Error::NotFound) => Ok(None),
            Err(e) => Err(e),
        }
    }

    async fn get_all(&self) -> Result<Vec<TState>, Error> {
        let dir_path = self.get_projection_path();
        let files = self.storage.read_dir(&dir_path).await.unwrap_or_default();

        let mut results = Vec::new();
        for file_path in files {
            if file_path.ends_with(".json")
                && let Ok(data) = self.storage.read_file(&file_path).await
            {
                if let Ok(wrapper) = serde_json::from_slice::<ProjectionWrapper<TState>>(&data) {
                    results.push(wrapper.data);
                } else if let Ok(state) = serde_json::from_slice::<TState>(&data) {
                    results.push(state);
                }
            }
        }
        Ok(results)
    }

    async fn save(&self, key: &str, state: &TState) -> Result<(), Error> {
        if let Some(provider) = &self.tag_provider
            && let Ok(Some(old_state)) = self.get(key).await
        {
            let old_tags = provider.get_tags(&old_state);
            for tag in old_tags {
                let dir_path = alloc::format!(
                    "{}/Indices/{}/{}",
                    self.get_projection_path(),
                    tag.key.to_lowercase(),
                    tag.value.to_lowercase()
                );
                let file_path = alloc::format!("{}/{}.json", dir_path, key);
                let _ = self.storage.delete_file(&file_path).await;
            }
        }

        let dir_path = self.get_projection_path();
        let _ = self.storage.create_dir_all(&dir_path).await;

        let metadata_dir = alloc::format!("{}/Metadata", self.get_projection_path());
        let _ = self.storage.create_dir_all(&metadata_dir).await;

        let path = self.get_file_path(key);
        let now = if let Some(clock) = &self.clock {
            crate::ports::Clock::now_millis(clock)
        } else {
            0
        };

        let metadata_index_path = alloc::format!("{}/index.json", metadata_dir);
        let mut metadata_index: alloc::collections::BTreeMap<
            alloc::string::String,
            ProjectionMetadata,
        > = if let Ok(data) = self.storage.read_file(&metadata_index_path).await {
            serde_json::from_slice(&data).unwrap_or_default()
        } else {
            alloc::collections::BTreeMap::new()
        };

        let mut current_metadata = if let Some(meta) = metadata_index.get(key) {
            let mut m = meta.clone();
            m.version += 1;
            m.last_updated_at = now;
            m
        } else {
            ProjectionMetadata {
                created_at: now,
                last_updated_at: now,
                version: 1,
                size_in_bytes: 0,
            }
        };

        let wrapper = ProjectionWrapper {
            data: state.clone(),
            metadata: current_metadata.clone(),
        };

        let data = serde_json::to_vec(&wrapper).map_err(|_| Error::IoError)?;
        current_metadata.size_in_bytes = data.len() as u64;

        self.storage.write_file(&path, data.as_slice()).await?;

        metadata_index.insert(alloc::string::String::from(key), current_metadata.clone());

        if let Ok(index_data) = serde_json::to_vec(&metadata_index) {
            let _ = self
                .storage
                .write_file(&metadata_index_path, index_data.as_slice())
                .await;
        }

        if let Some(provider) = &self.tag_provider {
            let new_tags = provider.get_tags(state);
            for tag in new_tags {
                let tag_dir = alloc::format!(
                    "{}/Indices/{}/{}",
                    self.get_projection_path(),
                    tag.key.to_lowercase(),
                    tag.value.to_lowercase()
                );
                let _ = self.storage.create_dir_all(&tag_dir).await;
                let tag_file = alloc::format!("{}/{}.json", tag_dir, key);
                let _ = self.storage.write_file(&tag_file, b"").await;
            }
        }
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<(), Error> {
        if let Some(provider) = &self.tag_provider
            && let Ok(Some(old_state)) = self.get(key).await
        {
            let old_tags = provider.get_tags(&old_state);
            for tag in old_tags {
                let dir_path = alloc::format!(
                    "{}/Indices/{}/{}",
                    self.get_projection_path(),
                    tag.key.to_lowercase(),
                    tag.value.to_lowercase()
                );
                let file_path = alloc::format!("{}/{}.json", dir_path, key);
                let _ = self.storage.delete_file(&file_path).await;
            }
        }

        let metadata_index_path =
            alloc::format!("{}/Metadata/index.json", self.get_projection_path());
        if let Ok(data) = self.storage.read_file(&metadata_index_path).await {
            if let Ok(mut metadata_index) = serde_json::from_slice::<
                alloc::collections::BTreeMap<alloc::string::String, ProjectionMetadata>,
            >(&data)
            {
                if metadata_index.remove(key).is_some() {
                    if let Ok(index_data) = serde_json::to_vec(&metadata_index) {
                        let _ = self
                            .storage
                            .write_file(&metadata_index_path, index_data.as_slice())
                            .await;
                    }
                }
            }
        }

        let path = self.get_file_path(key);
        match self.storage.delete_file(&path).await {
            Ok(_) => Ok(()),
            Err(Error::NotFound) => Ok(()),
            Err(e) => Err(e),
        }
    }
}

use crate::domain::Tag;
use alloc::collections::BTreeSet;

pub struct ProjectionTagIndex<S> {
    storage: S,
}

impl<S: StorageBackend + Send + Sync> ProjectionTagIndex<S> {
    pub fn new(storage: S) -> Self {
        Self { storage }
    }

    fn get_index_path(&self, root_path: &str, tag: &Tag) -> String {
        let tag_key = tag.key.to_ascii_lowercase();
        let tag_value = tag.value.to_ascii_lowercase();
        alloc::format!("{}/Indices/{}_{}.json", root_path, tag_key, tag_value)
    }

    fn get_tag_lock_key(&self, root_path: &str, tag: &Tag) -> String {
        alloc::format!(
            "{}|{}|{}",
            root_path,
            tag.key.to_ascii_lowercase(),
            tag.value.to_ascii_lowercase()
        )
    }

    pub async fn add_projection(
        &self,
        root_path: &str,
        tag: &Tag,
        projection_key: &str,
    ) -> Result<(), Error> {
        let indices_dir = alloc::format!("{}/Indices", root_path);
        let _ = self.storage.create_dir_all(&indices_dir).await;

        let index_path = self.get_index_path(root_path, tag);
        let lock_key = self.get_tag_lock_key(root_path, tag);

        self.storage.acquire_stream_lock(&lock_key).await?;

        let mut keys = self.read_keys(&index_path).await.unwrap_or_default();
        if !keys.contains(&alloc::string::String::from(projection_key)) {
            keys.push(alloc::string::String::from(projection_key));
            // C# implementation uses temp file + rename, here we'll just write directly.
            // In a real implementation this would need atomicity.
            if let Ok(data) = serde_json::to_vec(&keys) {
                let _ = self.storage.write_file(&index_path, &data).await;
            }
        }

        self.storage.release_stream_lock(&lock_key).await?;
        Ok(())
    }

    pub async fn remove_projection(
        &self,
        root_path: &str,
        tag: &Tag,
        projection_key: &str,
    ) -> Result<(), Error> {
        let index_path = self.get_index_path(root_path, tag);
        let lock_key = self.get_tag_lock_key(root_path, tag);

        // Return early if file doesn't exist to save locking
        if let Err(Error::NotFound) = self.storage.read_file(&index_path).await {
            return Ok(());
        }

        self.storage.acquire_stream_lock(&lock_key).await?;

        let mut keys = self.read_keys(&index_path).await.unwrap_or_default();
        let original_len = keys.len();
        keys.retain(|k| k != projection_key);

        if keys.len() != original_len {
            if keys.is_empty() {
                let _ = self.storage.delete_file(&index_path).await;
            } else if let Ok(data) = serde_json::to_vec(&keys) {
                let _ = self.storage.write_file(&index_path, &data).await;
            }
        }

        self.storage.release_stream_lock(&lock_key).await?;
        Ok(())
    }

    pub async fn get_projection_keys_by_tag(
        &self,
        root_path: &str,
        tag: &Tag,
    ) -> Result<Vec<String>, Error> {
        let index_path = self.get_index_path(root_path, tag);
        Ok(self.read_keys(&index_path).await.unwrap_or_default())
    }

    pub async fn get_projection_keys_by_tags(
        &self,
        root_path: &str,
        tags: &[Tag],
    ) -> Result<Vec<String>, Error> {
        if tags.is_empty() {
            return Ok(Vec::new());
        }

        let mut key_sets = Vec::new();
        for tag in tags {
            let keys = self.get_projection_keys_by_tag(root_path, tag).await?;
            if keys.is_empty() {
                return Ok(Vec::new());
            }
            key_sets.push(keys);
        }

        // Sort starting with smallest set for efficiency
        key_sets.sort_by_key(|s| s.len());

        let mut result_set: BTreeSet<_> = key_sets[0].iter().cloned().collect();
        for set in key_sets.iter().skip(1) {
            let next_set: BTreeSet<_> = set.iter().cloned().collect();
            result_set.retain(|k| next_set.contains(k));

            if result_set.is_empty() {
                return Ok(Vec::new());
            }
        }

        Ok(result_set.into_iter().collect())
    }

    pub async fn update_projection_tags(
        &self,
        root_path: &str,
        projection_key: &str,
        old_tags: &[Tag],
        new_tags: &[Tag],
    ) -> Result<(), Error> {
        // Find tags to remove (in old but not in new)
        for old_tag in old_tags {
            let matches_new = new_tags.iter().any(|t| {
                t.key.eq_ignore_ascii_case(&old_tag.key)
                    && t.value.eq_ignore_ascii_case(&old_tag.value)
            });
            if !matches_new {
                self.remove_projection(root_path, old_tag, projection_key)
                    .await?;
            }
        }

        // Find tags to add (in new but not in old)
        for new_tag in new_tags {
            let matches_old = old_tags.iter().any(|t| {
                t.key.eq_ignore_ascii_case(&new_tag.key)
                    && t.value.eq_ignore_ascii_case(&new_tag.value)
            });
            if !matches_old {
                self.add_projection(root_path, new_tag, projection_key)
                    .await?;
            }
        }

        Ok(())
    }

    pub async fn delete_all_indices(&self, root_path: &str) -> Result<(), Error> {
        let indices_dir = alloc::format!("{}/Indices", root_path);
        if let Ok(files) = self.storage.read_dir(&indices_dir).await {
            for file in files {
                let _ = self.storage.delete_file(&file).await;
            }
        }
        Ok(())
    }

    async fn read_keys(&self, path: &str) -> Result<Vec<String>, Error> {
        match self.storage.read_file(path).await {
            Ok(data) => {
                // If parsing fails, C# treats it as empty index.
                match serde_json::from_slice::<Vec<String>>(&data) {
                    Ok(keys) => Ok(keys),
                    Err(_) => Ok(Vec::new()),
                }
            }
            Err(Error::NotFound) => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Serialized projection payload that combines state and lifecycle metadata.
///
/// # Overview
///
/// The projection store persists this wrapper so it can read state and metadata from one file.
///
/// # Examples
///
/// ```rust
/// use marmosa::projections::{ProjectionMetadata, ProjectionWrapper};
///
/// let wrapped = ProjectionWrapper {
///     data: 42_u64,
///     metadata: ProjectionMetadata {
///         created_at: 1_000,
///         last_updated_at: 2_000,
///         version: 2,
///         size_in_bytes: 128,
///     },
/// };
///
/// assert_eq!(wrapped.data, 42);
/// assert_eq!(wrapped.metadata.version, 2);
/// ```
pub struct ProjectionWrapper<TState> {
    #[serde(rename = "data")]
    pub data: TState,
    #[serde(rename = "metadata")]
    pub metadata: ProjectionMetadata,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
/// Lifecycle metadata tracked for each projection entry.
///
/// # Overview
///
/// - `created_at`: first creation timestamp in milliseconds.
/// - `last_updated_at`: last update timestamp in milliseconds.
/// - `version`: number of successful saves for the key, starting at `1`.
/// - `size_in_bytes`: serialized projection file size.
///
/// # Examples
///
/// ```rust
/// use marmosa::projections::ProjectionMetadata;
///
/// let metadata = ProjectionMetadata {
///     created_at: 1_000,
///     last_updated_at: 1_500,
///     version: 1,
///     size_in_bytes: 256,
/// };
///
/// assert_eq!(metadata.version, 1);
/// assert!(metadata.last_updated_at >= metadata.created_at);
/// ```
pub struct ProjectionMetadata {
    pub created_at: u64,
    pub last_updated_at: u64,
    pub version: u64,
    pub size_in_bytes: u64,
}

/// Metadata index accessor for projection entries.
///
/// # Overview
///
/// The index is stored at `{root_path}/Metadata/index.json` and allows metadata queries without
/// reading each projection state file.
pub struct ProjectionMetadataIndex<S> {
    storage: S,
}

impl<S: crate::ports::StorageBackend + Send + Sync> ProjectionMetadataIndex<S> {
    /// Creates a new metadata index accessor using the given storage backend.
    pub fn new(storage: S) -> Self {
        Self { storage }
    }

    fn get_index_path(&self, root_path: &str) -> alloc::string::String {
        alloc::format!("{}/Metadata/index.json", root_path)
    }

    fn get_lock_key(&self, root_path: &str) -> alloc::string::String {
        alloc::format!("metadata_lock:{}", root_path)
    }

    /// Inserts or updates metadata for a projection key.
    ///
    /// # Errors
    ///
    /// Returns an error if lock acquisition fails.
    /// Storage write failures are currently best-effort and may be ignored by this implementation.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use marmosa::ports::{Error, StorageBackend};
    /// use marmosa::projections::{ProjectionMetadata, ProjectionMetadataIndex};
    ///
    /// async fn upsert_metadata<S: StorageBackend + Send + Sync>(storage: S) -> Result<(), Error> {
    ///     let index = ProjectionMetadataIndex::new(storage);
    ///     index
    ///         .save(
    ///             "/db/Projections/StudentSummary",
    ///             "student-1",
    ///             ProjectionMetadata {
    ///                 created_at: 1_000,
    ///                 last_updated_at: 1_500,
    ///                 version: 1,
    ///                 size_in_bytes: 320,
    ///             },
    ///         )
    ///         .await
    /// }
    /// ```
    pub async fn save(
        &self,
        root_path: &str,
        key: &str,
        metadata: ProjectionMetadata,
    ) -> Result<(), crate::ports::Error> {
        let _ = self
            .storage
            .create_dir_all(&alloc::format!("{}/Metadata", root_path))
            .await;

        let index_path = self.get_index_path(root_path);
        let lock_key = self.get_lock_key(root_path);

        self.storage.acquire_stream_lock(&lock_key).await?;

        let mut index = self.read_index(&index_path).await.unwrap_or_default();
        index.insert(alloc::string::String::from(key), metadata);

        if let Ok(data) = serde_json::to_vec(&index) {
            let _ = self.storage.write_file(&index_path, &data).await;
        }

        self.storage.release_stream_lock(&lock_key).await?;
        Ok(())
    }

    /// Loads metadata for a single projection key.
    ///
    /// # Errors
    ///
    /// Returns an error when storage access fails.
    pub async fn get(
        &self,
        root_path: &str,
        key: &str,
    ) -> Result<Option<ProjectionMetadata>, crate::ports::Error> {
        let index_path = self.get_index_path(root_path);
        let index = self.read_index(&index_path).await.unwrap_or_default();
        Ok(index.get(key).cloned())
    }

    /// Returns the full metadata map for a projection root.
    ///
    /// # Errors
    ///
    /// Returns an error when storage access fails.
    pub async fn get_all(
        &self,
        root_path: &str,
    ) -> Result<
        alloc::collections::BTreeMap<alloc::string::String, ProjectionMetadata>,
        crate::ports::Error,
    > {
        let index_path = self.get_index_path(root_path);
        Ok(self.read_index(&index_path).await.unwrap_or_default())
    }

    /// Returns metadata entries updated at or after `cutoff_time`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use marmosa::ports::{Error, StorageBackend};
    /// use marmosa::projections::ProjectionMetadataIndex;
    ///
    /// async fn recent_keys<S: StorageBackend + Send + Sync>(storage: S) -> Result<Vec<String>, Error> {
    ///     let index = ProjectionMetadataIndex::new(storage);
    ///     let entries = index
    ///         .get_updated_since("/db/Projections/StudentSummary", 1_700_000_000_000)
    ///         .await?;
    ///
    ///     Ok(entries.into_iter().map(|(key, _)| key).collect())
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error when storage access fails.
    pub async fn get_updated_since(
        &self,
        root_path: &str,
        cutoff_time: u64,
    ) -> Result<alloc::vec::Vec<(alloc::string::String, ProjectionMetadata)>, crate::ports::Error>
    {
        let index_path = self.get_index_path(root_path);
        let index = self.read_index(&index_path).await.unwrap_or_default();

        let mut result = alloc::vec::Vec::new();
        for (k, v) in index {
            if v.last_updated_at >= cutoff_time {
                result.push((k, v));
            }
        }
        Ok(result)
    }

    /// Removes metadata for a projection key.
    ///
    /// # Errors
    ///
    /// Returns an error if lock acquisition fails.
    pub async fn delete(&self, root_path: &str, key: &str) -> Result<(), crate::ports::Error> {
        let index_path = self.get_index_path(root_path);
        let lock_key = self.get_lock_key(root_path);

        if let Err(crate::ports::Error::NotFound) = self.storage.read_file(&index_path).await {
            return Ok(());
        }

        self.storage.acquire_stream_lock(&lock_key).await?;

        let mut index = self.read_index(&index_path).await.unwrap_or_default();
        if index.remove(key).is_some() {
            if index.is_empty() {
                let _ = self.storage.delete_file(&index_path).await;
            } else if let Ok(data) = serde_json::to_vec(&index) {
                let _ = self.storage.write_file(&index_path, &data).await;
            }
        }

        self.storage.release_stream_lock(&lock_key).await?;
        Ok(())
    }

    /// Removes the metadata index file for the projection root.
    ///
    /// # Errors
    ///
    /// Returns an error if lock acquisition fails.
    pub async fn clear(&self, root_path: &str) -> Result<(), crate::ports::Error> {
        let index_path = self.get_index_path(root_path);
        let lock_key = self.get_lock_key(root_path);

        self.storage.acquire_stream_lock(&lock_key).await?;
        let _ = self.storage.delete_file(&index_path).await;
        self.storage.release_stream_lock(&lock_key).await?;
        Ok(())
    }

    async fn read_index(
        &self,
        path: &str,
    ) -> Result<
        alloc::collections::BTreeMap<alloc::string::String, ProjectionMetadata>,
        crate::ports::Error,
    > {
        match self.storage.read_file(path).await {
            Ok(data) => {
                match serde_json::from_slice::<alloc::collections::BTreeMap<&str, ProjectionMetadata>>(
                    &data,
                ) {
                    Ok(index) => {
                        let mut string_index = alloc::collections::BTreeMap::new();
                        for (k, v) in index {
                            string_index.insert(alloc::string::String::from(k), v);
                        }
                        Ok(string_index)
                    }
                    Err(_) => Ok(alloc::collections::BTreeMap::new()),
                }
            }
            Err(crate::ports::Error::NotFound) => Ok(alloc::collections::BTreeMap::new()),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_projection_metadata_can_be_created() {
        let metadata = ProjectionMetadata {
            created_at: 1000,
            last_updated_at: 1000,
            version: 1,
            size_in_bytes: 256,
        };
        assert_eq!(metadata.created_at, 1000);
        assert_eq!(metadata.last_updated_at, 1000);
        assert_eq!(metadata.version, 1);
        assert_eq!(metadata.size_in_bytes, 256);
    }

    #[test]
    fn test_projection_metadata_supports_with_syntax() {
        let original = ProjectionMetadata {
            created_at: 1000,
            last_updated_at: 2000,
            version: 5,
            size_in_bytes: 512,
        };
        let updated = ProjectionMetadata {
            last_updated_at: 3000,
            version: 6,
            size_in_bytes: 600,
            ..original
        };
        assert_eq!(updated.created_at, 1000);
        assert_ne!(original.last_updated_at, updated.last_updated_at);
        assert_eq!(updated.version, 6);
        assert_eq!(updated.size_in_bytes, 600);
    }

    #[tokio::test]
    async fn test_projection_metadata_index_save() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let index = ProjectionMetadataIndex::new(storage.clone());
        let m = ProjectionMetadata {
            created_at: 1000,
            last_updated_at: 1000,
            version: 1,
            size_in_bytes: 256,
        };
        index.save("/temp", "k1", m.clone()).await.unwrap();
        assert!(storage.read_file("/temp/Metadata/index.json").await.is_ok());
        let fetched = index.get("/temp", "k1").await.unwrap().unwrap();
        assert_eq!(fetched.version, 1);
    }

    #[tokio::test]
    async fn test_projection_metadata_index_get_all() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let index = ProjectionMetadataIndex::new(storage.clone());
        index
            .save(
                "/temp",
                "k1",
                ProjectionMetadata {
                    created_at: 1,
                    last_updated_at: 1,
                    version: 1,
                    size_in_bytes: 1,
                },
            )
            .await
            .unwrap();
        index
            .save(
                "/temp",
                "k2",
                ProjectionMetadata {
                    created_at: 2,
                    last_updated_at: 2,
                    version: 2,
                    size_in_bytes: 2,
                },
            )
            .await
            .unwrap();
        let all = index.get_all("/temp").await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn test_projection_metadata_index_get_updated_since() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let index = ProjectionMetadataIndex::new(storage.clone());
        index
            .save(
                "/temp",
                "k1",
                ProjectionMetadata {
                    created_at: 1,
                    last_updated_at: 1,
                    version: 1,
                    size_in_bytes: 1,
                },
            )
            .await
            .unwrap();
        index
            .save(
                "/temp",
                "k2",
                ProjectionMetadata {
                    created_at: 2,
                    last_updated_at: 10,
                    version: 2,
                    size_in_bytes: 2,
                },
            )
            .await
            .unwrap();
        let recent = index.get_updated_since("/temp", 5).await.unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].0, "k2");
    }

    #[tokio::test]
    async fn test_projection_metadata_index_delete() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let index = ProjectionMetadataIndex::new(storage.clone());
        index
            .save(
                "/temp",
                "k1",
                ProjectionMetadata {
                    created_at: 1,
                    last_updated_at: 1,
                    version: 1,
                    size_in_bytes: 1,
                },
            )
            .await
            .unwrap();
        index.delete("/temp", "k1").await.unwrap();
        let fetched = index.get("/temp", "k1").await.unwrap();
        assert!(fetched.is_none());
    }

    #[tokio::test]
    async fn test_projection_metadata_index_clear() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let index = ProjectionMetadataIndex::new(storage.clone());
        index
            .save(
                "/temp",
                "k1",
                ProjectionMetadata {
                    created_at: 1,
                    last_updated_at: 1,
                    version: 1,
                    size_in_bytes: 1,
                },
            )
            .await
            .unwrap();
        index.clear("/temp").await.unwrap();
        let all = index.get_all("/temp").await.unwrap();
        assert!(all.is_empty());
    }

    use super::*;
    use crate::domain::{DomainEvent, Tag};
    use crate::ports::tests::InMemoryStorage;
    use alloc::string::ToString;
    use alloc::vec;

    #[tokio::test]
    async fn projection_tag_index_add_projection_creates_index_file() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let index = ProjectionTagIndex::new(storage.clone());
        let tag = Tag {
            key: "Status".to_string(),
            value: "Active".to_string(),
        };
        let projection_key = "proj-1";

        index
            .add_projection("/temp", &tag, projection_key)
            .await
            .unwrap();

        let index_file = "/temp/Indices/status_active.json";
        assert!(storage.read_file(index_file).await.is_ok());
    }

    #[tokio::test]
    async fn projection_tag_index_add_projection_adds_key_to_existing_index() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let index = ProjectionTagIndex::new(storage.clone());
        let tag = Tag {
            key: "Status".to_string(),
            value: "Active".to_string(),
        };

        index.add_projection("/temp", &tag, "proj-1").await.unwrap();
        index.add_projection("/temp", &tag, "proj-2").await.unwrap();
        index.add_projection("/temp", &tag, "proj-3").await.unwrap();

        let keys = index
            .get_projection_keys_by_tag("/temp", &tag)
            .await
            .unwrap();
        assert_eq!(keys.len(), 3);
        assert!(keys.contains(&"proj-1".to_string()));
        assert!(keys.contains(&"proj-2".to_string()));
        assert!(keys.contains(&"proj-3".to_string()));
    }

    #[tokio::test]
    async fn projection_tag_index_add_projection_prevents_duplicates() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let index = ProjectionTagIndex::new(storage.clone());
        let tag = Tag {
            key: "Status".to_string(),
            value: "Active".to_string(),
        };

        index.add_projection("/temp", &tag, "proj-1").await.unwrap();
        index.add_projection("/temp", &tag, "proj-1").await.unwrap();

        let keys = index
            .get_projection_keys_by_tag("/temp", &tag)
            .await
            .unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0], "proj-1");
    }

    #[tokio::test]
    async fn projection_tag_index_remove_projection_removes_key_from_index() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let index = ProjectionTagIndex::new(storage.clone());
        let tag = Tag {
            key: "Status".to_string(),
            value: "Active".to_string(),
        };

        index.add_projection("/temp", &tag, "proj-1").await.unwrap();
        index.add_projection("/temp", &tag, "proj-2").await.unwrap();

        index
            .remove_projection("/temp", &tag, "proj-1")
            .await
            .unwrap();

        let keys = index
            .get_projection_keys_by_tag("/temp", &tag)
            .await
            .unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0], "proj-2");
    }

    #[tokio::test]
    async fn projection_tag_index_remove_projection_deletes_index_file_when_empty() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let index = ProjectionTagIndex::new(storage.clone());
        let tag = Tag {
            key: "Status".to_string(),
            value: "Active".to_string(),
        };

        index.add_projection("/temp", &tag, "proj-1").await.unwrap();
        index
            .remove_projection("/temp", &tag, "proj-1")
            .await
            .unwrap();

        let index_file = "/temp/Indices/status_active.json";
        assert!(storage.read_file(index_file).await.is_err());
    }

    #[tokio::test]
    async fn projection_tag_index_get_projection_keys_by_tag_returns_empty_for_non_existent() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let index = ProjectionTagIndex::new(storage.clone());
        let tag = Tag {
            key: "Status".to_string(),
            value: "Inactive".to_string(),
        };

        let keys = index
            .get_projection_keys_by_tag("/temp", &tag)
            .await
            .unwrap();
        assert!(keys.is_empty());
    }

    #[tokio::test]
    async fn projection_tag_index_get_projection_keys_by_tags_returns_intersection() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let index = ProjectionTagIndex::new(storage.clone());
        let tag1 = Tag {
            key: "Status".to_string(),
            value: "Active".to_string(),
        };
        let tag2 = Tag {
            key: "Tier".to_string(),
            value: "Premium".to_string(),
        };

        index
            .add_projection("/temp", &tag1, "proj-1")
            .await
            .unwrap();
        index
            .add_projection("/temp", &tag1, "proj-2")
            .await
            .unwrap();
        index
            .add_projection("/temp", &tag1, "proj-3")
            .await
            .unwrap();

        index
            .add_projection("/temp", &tag2, "proj-1")
            .await
            .unwrap();
        index
            .add_projection("/temp", &tag2, "proj-3")
            .await
            .unwrap();

        let tags = alloc::vec![tag1, tag2];
        let keys = index
            .get_projection_keys_by_tags("/temp", &tags)
            .await
            .unwrap();

        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&"proj-1".to_string()));
        assert!(keys.contains(&"proj-3".to_string()));
    }

    #[tokio::test]
    async fn projection_tag_index_get_projection_keys_by_tags_returns_empty_if_any_tag_has_no_matches()
     {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let index = ProjectionTagIndex::new(storage.clone());
        let tag1 = Tag {
            key: "Status".to_string(),
            value: "Active".to_string(),
        };
        let tag2 = Tag {
            key: "NonExistent".to_string(),
            value: "Value".to_string(),
        };

        index
            .add_projection("/temp", &tag1, "proj-1")
            .await
            .unwrap();

        let tags = alloc::vec![tag1, tag2];
        let keys = index
            .get_projection_keys_by_tags("/temp", &tags)
            .await
            .unwrap();

        assert!(keys.is_empty());
    }

    #[tokio::test]
    async fn projection_tag_index_update_projection_tags_removes_old_and_adds_new() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let index = ProjectionTagIndex::new(storage.clone());
        let proj_key = "proj-1";

        let old_tags = alloc::vec![
            Tag {
                key: "Status".to_string(),
                value: "Pending".to_string()
            },
            Tag {
                key: "Tier".to_string(),
                value: "Basic".to_string()
            }
        ];

        let new_tags = alloc::vec![
            Tag {
                key: "Status".to_string(),
                value: "Active".to_string()
            },
            Tag {
                key: "Tier".to_string(),
                value: "Premium".to_string()
            }
        ];

        for tag in &old_tags {
            index.add_projection("/temp", tag, proj_key).await.unwrap();
        }

        index
            .update_projection_tags("/temp", proj_key, &old_tags, &new_tags)
            .await
            .unwrap();

        let pending_keys = index
            .get_projection_keys_by_tag("/temp", &old_tags[0])
            .await
            .unwrap();
        assert!(pending_keys.is_empty());

        let basic_keys = index
            .get_projection_keys_by_tag("/temp", &old_tags[1])
            .await
            .unwrap();
        assert!(basic_keys.is_empty());

        let active_keys = index
            .get_projection_keys_by_tag("/temp", &new_tags[0])
            .await
            .unwrap();
        assert!(active_keys.contains(&proj_key.to_string()));

        let premium_keys = index
            .get_projection_keys_by_tag("/temp", &new_tags[1])
            .await
            .unwrap();
        assert!(premium_keys.contains(&proj_key.to_string()));
    }

    #[tokio::test]
    async fn projection_tag_index_update_projection_tags_only_updates_changed_tags() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let index = ProjectionTagIndex::new(storage.clone());
        let proj_key = "proj-1";

        let old_tags = alloc::vec![
            Tag {
                key: "Status".to_string(),
                value: "Active".to_string()
            },
            Tag {
                key: "Tier".to_string(),
                value: "Basic".to_string()
            }
        ];

        let new_tags = alloc::vec![
            Tag {
                key: "Status".to_string(),
                value: "Active".to_string()
            }, // Unchanged
            Tag {
                key: "Tier".to_string(),
                value: "Premium".to_string()
            } // Changed
        ];

        for tag in &old_tags {
            index.add_projection("/temp", tag, proj_key).await.unwrap();
        }

        index
            .update_projection_tags("/temp", proj_key, &old_tags, &new_tags)
            .await
            .unwrap();

        let active_keys = index
            .get_projection_keys_by_tag("/temp", &new_tags[0])
            .await
            .unwrap();
        assert!(active_keys.contains(&proj_key.to_string()));

        let basic_keys = index
            .get_projection_keys_by_tag("/temp", &old_tags[1])
            .await
            .unwrap();
        assert!(basic_keys.is_empty());

        let premium_keys = index
            .get_projection_keys_by_tag("/temp", &new_tags[1])
            .await
            .unwrap();
        assert!(premium_keys.contains(&proj_key.to_string()));
    }

    #[tokio::test]
    async fn projection_tag_index_delete_all_indices_removes_indices() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let index = ProjectionTagIndex::new(storage.clone());
        let tag = Tag {
            key: "Status".to_string(),
            value: "Active".to_string(),
        };

        index.add_projection("/temp", &tag, "proj-1").await.unwrap();

        index.delete_all_indices("/temp").await.unwrap();

        let index_file = "/temp/Indices/status_active.json";
        assert!(storage.read_file(index_file).await.is_err());
    }

    #[tokio::test]
    async fn projection_tag_index_concurrent_addition_same_tag_no_lost_updates() {
        let storage = alloc::sync::Arc::new(InMemoryStorage::new());
        let index = alloc::sync::Arc::new(ProjectionTagIndex::new(storage.clone()));
        let tag = alloc::sync::Arc::new(Tag {
            key: "Status".to_string(),
            value: "Active".to_string(),
        });

        let mut handles = alloc::vec::Vec::new();

        for i in 0..50 {
            let idx = index.clone();
            let t = tag.clone();
            let key = alloc::format!("proj-{}", i);
            handles.push(tokio::spawn(async move {
                idx.add_projection("/temp_p", &t, &key).await.unwrap();
            }));
        }

        for handle in handles {
            handle.await.unwrap();
        }

        let keys = index
            .get_projection_keys_by_tag("/temp_p", &tag)
            .await
            .unwrap();
        assert_eq!(keys.len(), 50);
    }

    // A simple counter projection for testing
    #[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
    struct CounterState {
        key: String,
        count: u64,
    }

    struct CounterProjection;

    impl ProjectionDefinition for CounterProjection {
        type State = CounterState;

        fn projection_name(&self) -> &str {
            "CounterProjection"
        }

        fn event_types(&self) -> Query {
            Query {
                items: vec![crate::domain::QueryItem {
                    event_types: vec!["CounterIncremented".to_string()],
                    tags: vec![],
                }],
            }
        }

        fn key_selector(&self, event: &EventRecord) -> Option<String> {
            // Extract key from tags
            event
                .event
                .tags
                .iter()
                .find(|t| t.key == "counter_id")
                .map(|t| t.value.clone())
        }

        fn apply(&self, state: Option<Self::State>, event: &EventRecord) -> Option<Self::State> {
            let key = self.key_selector(event)?;
            let mut current = state.unwrap_or_else(|| CounterState {
                key: key.clone(),
                count: 0,
            });

            if event.event.event_type == "CounterIncremented" {
                current.count += 1;
            }

            Some(current)
        }
    }

    #[test]
    fn test_projection_definition_returns_name() {
        let projection = CounterProjection;
        assert_eq!(projection.projection_name(), "CounterProjection");
    }

    #[test]
    fn test_projection_definition_returns_event_types_query() {
        let projection = CounterProjection;
        let query = projection.event_types();
        assert_eq!(query.items.len(), 1);
        assert_eq!(
            query.items[0].event_types,
            vec!["CounterIncremented".to_string()]
        );
    }

    #[test]
    fn test_projection_apply_creates_new_state() {
        let projection = CounterProjection;

        let event = EventRecord {
            position: 0,
            event_id: "evt-1".to_string(),
            event: DomainEvent {
                event_type: "CounterIncremented".to_string(),
                data: "{}".to_string(),
                tags: vec![Tag {
                    key: "counter_id".to_string(),
                    value: "counter-1".to_string(),
                }],
            },
            metadata: None,
            timestamp: 1000,
        };

        let new_state = projection.apply(None, &event);

        assert!(new_state.is_some());
        let state = new_state.unwrap();
        assert_eq!(state.key, "counter-1");
        assert_eq!(state.count, 1);
    }

    #[test]
    fn test_projection_apply_updates_existing_state() {
        let projection = CounterProjection;

        let existing = CounterState {
            key: "counter-1".to_string(),
            count: 5,
        };

        let event = EventRecord {
            position: 1,
            event_id: "evt-2".to_string(),
            event: DomainEvent {
                event_type: "CounterIncremented".to_string(),
                data: "{}".to_string(),
                tags: vec![Tag {
                    key: "counter_id".to_string(),
                    value: "counter-1".to_string(),
                }],
            },
            metadata: None,
            timestamp: 2000,
        };

        let new_state = projection.apply(Some(existing), &event);

        assert!(new_state.is_some());
        let state = new_state.unwrap();
        assert_eq!(state.count, 6);
    }

    #[test]
    fn test_key_selector_extracts_from_tags() {
        let projection = CounterProjection;

        let event = EventRecord {
            position: 0,
            event_id: "evt-1".to_string(),
            event: DomainEvent {
                event_type: "CounterIncremented".to_string(),
                data: "{}".to_string(),
                tags: vec![Tag {
                    key: "counter_id".to_string(),
                    value: "my-counter".to_string(),
                }],
            },
            metadata: None,
            timestamp: 1000,
        };

        let key = projection.key_selector(&event);
        assert_eq!(key, Some("my-counter".to_string()));
    }

    #[test]
    fn test_key_selector_returns_none_when_tag_missing() {
        let projection = CounterProjection;

        let event = EventRecord {
            position: 0,
            event_id: "evt-1".to_string(),
            event: DomainEvent {
                event_type: "CounterIncremented".to_string(),
                data: "{}".to_string(),
                tags: vec![], // No tags
            },
            metadata: None,
            timestamp: 1000,
        };

        let key = projection.key_selector(&event);
        assert_eq!(key, None);
    }

    // StorageBackendProjectionStore tests

    #[tokio::test]
    async fn test_projection_store_save_and_get() {
        let storage = InMemoryStorage::new();
        let store: StorageBackendProjectionStore<_, CounterState> =
            StorageBackendProjectionStore::new(storage, "CounterProjection".to_string());

        let state = CounterState {
            key: "counter-1".to_string(),
            count: 42,
        };
        store.save("counter-1", &state).await.unwrap();

        let retrieved = store.get("counter-1").await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().count, 42);
    }

    #[tokio::test]
    async fn test_projection_store_get_returns_none_for_missing() {
        let storage = InMemoryStorage::new();
        let store: StorageBackendProjectionStore<_, CounterState> =
            StorageBackendProjectionStore::new(storage, "CounterProjection".to_string());

        let retrieved = store.get("nonexistent").await.unwrap();
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_projection_store_delete() {
        let storage = InMemoryStorage::new();
        let store: StorageBackendProjectionStore<_, CounterState> =
            StorageBackendProjectionStore::new(storage, "CounterProjection".to_string());

        let state = CounterState {
            key: "counter-1".to_string(),
            count: 10,
        };
        store.save("counter-1", &state).await.unwrap();

        // Verify it exists
        assert!(store.get("counter-1").await.unwrap().is_some());

        // Delete it
        store.delete("counter-1").await.unwrap();

        // Verify it's gone
        assert!(store.get("counter-1").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_projection_store_get_all() {
        let storage = InMemoryStorage::new();
        let store: StorageBackendProjectionStore<_, CounterState> =
            StorageBackendProjectionStore::new(storage, "CounterProjection".to_string());

        store
            .save(
                "counter-1",
                &CounterState {
                    key: "counter-1".to_string(),
                    count: 1,
                },
            )
            .await
            .unwrap();
        store
            .save(
                "counter-2",
                &CounterState {
                    key: "counter-2".to_string(),
                    count: 2,
                },
            )
            .await
            .unwrap();
        store
            .save(
                "counter-3",
                &CounterState {
                    key: "counter-3".to_string(),
                    count: 3,
                },
            )
            .await
            .unwrap();

        let all = store.get_all().await.unwrap();
        assert_eq!(all.len(), 3);

        // Verify total count
        let total: u64 = all.iter().map(|s| s.count).sum();
        assert_eq!(total, 6);
    }

    // ProjectionCheckpoint tests (Ported from C#)

    #[test]
    fn test_checkpoint_constructor_initializes_properties() {
        let checkpoint = ProjectionCheckpoint::default();
        assert_eq!(checkpoint.projection_name, "");
        assert_eq!(checkpoint.last_position, 0);
        assert_eq!(checkpoint.last_updated, 0);
        assert_eq!(checkpoint.total_events_processed, 0);
    }

    #[test]
    fn test_checkpoint_projection_name_can_be_set() {
        let mut checkpoint = ProjectionCheckpoint::default();
        checkpoint.projection_name = "TestProjection".to_string();
        assert_eq!(checkpoint.projection_name, "TestProjection");
    }

    #[test]
    fn test_checkpoint_last_position_can_be_set() {
        let mut checkpoint = ProjectionCheckpoint::default();
        checkpoint.last_position = 12345;
        assert_eq!(checkpoint.last_position, 12345);
    }

    #[test]
    fn test_checkpoint_last_updated_can_be_set() {
        let mut checkpoint = ProjectionCheckpoint::default();
        let timestamp = 1678886400000; // arbitrary timestamp
        checkpoint.last_updated = timestamp;
        assert_eq!(checkpoint.last_updated, timestamp);
    }

    #[test]
    fn test_checkpoint_total_events_processed_can_be_set() {
        let mut checkpoint = ProjectionCheckpoint::default();
        checkpoint.total_events_processed = 99999;
        assert_eq!(checkpoint.total_events_processed, 99999);
    }

    #[test]
    fn test_checkpoint_can_be_fully_populated() {
        let timestamp = 1678886400000;
        let checkpoint = ProjectionCheckpoint {
            projection_name: "OrderSummary".to_string(),
            last_position: 5000,
            last_updated: timestamp,
            total_events_processed: 5000,
        };

        assert_eq!(checkpoint.projection_name, "OrderSummary");
        assert_eq!(checkpoint.last_position, 5000);
        assert_eq!(checkpoint.last_updated, timestamp);
        assert_eq!(checkpoint.total_events_processed, 5000);
    }

    // ProjectionRunner tests

    #[tokio::test]
    async fn test_projection_runner_processes_events() {
        let storage = InMemoryStorage::new();
        let store: StorageBackendProjectionStore<InMemoryStorage, CounterState> =
            StorageBackendProjectionStore::new(
                InMemoryStorage::new(),
                "CounterProjection".to_string(),
            );

        let runner = ProjectionRunner::new(storage, CounterProjection, store);

        let events = vec![
            EventRecord {
                position: 1,
                event_id: "evt-1".to_string(),
                event: DomainEvent {
                    event_type: "CounterIncremented".to_string(),
                    data: "{}".to_string(),
                    tags: vec![Tag {
                        key: "counter_id".to_string(),
                        value: "counter-1".to_string(),
                    }],
                },
                metadata: None,
                timestamp: 1000,
            },
            EventRecord {
                position: 2,
                event_id: "evt-2".to_string(),
                event: DomainEvent {
                    event_type: "CounterIncremented".to_string(),
                    data: "{}".to_string(),
                    tags: vec![Tag {
                        key: "counter_id".to_string(),
                        value: "counter-1".to_string(),
                    }],
                },
                metadata: None,
                timestamp: 2000,
            },
        ];

        let last_pos = runner.process_events(&events).await.unwrap();
        assert_eq!(last_pos, 2);

        // Verify the state was updated
        let state = runner.store.get("counter-1").await.unwrap();
        assert!(state.is_some());
        assert_eq!(state.unwrap().count, 2);
    }

    #[tokio::test]
    async fn test_projection_runner_saves_checkpoint() {
        let storage = InMemoryStorage::new();
        let store: StorageBackendProjectionStore<InMemoryStorage, CounterState> =
            StorageBackendProjectionStore::new(
                InMemoryStorage::new(),
                "CounterProjection".to_string(),
            );

        let runner = ProjectionRunner::new(storage, CounterProjection, store);

        let events = vec![EventRecord {
            position: 5,
            event_id: "evt-5".to_string(),
            event: DomainEvent {
                event_type: "CounterIncremented".to_string(),
                data: "{}".to_string(),
                tags: vec![Tag {
                    key: "counter_id".to_string(),
                    value: "counter-1".to_string(),
                }],
            },
            metadata: None,
            timestamp: 5000,
        }];

        runner.process_events(&events).await.unwrap();

        // Verify checkpoint was saved
        let checkpoint = runner.get_checkpoint().await.unwrap().unwrap();
        assert_eq!(checkpoint.last_position, 5);
        assert_eq!(checkpoint.total_events_processed, 1);
    }

    #[tokio::test]
    async fn test_projection_runner_skips_already_processed_events() {
        let storage = InMemoryStorage::new();
        let store: StorageBackendProjectionStore<InMemoryStorage, CounterState> =
            StorageBackendProjectionStore::new(
                InMemoryStorage::new(),
                "CounterProjection".to_string(),
            );

        let runner = ProjectionRunner::new(storage, CounterProjection, store);

        // First batch
        let events1 = vec![EventRecord {
            position: 1,
            event_id: "evt-1".to_string(),
            event: DomainEvent {
                event_type: "CounterIncremented".to_string(),
                data: "{}".to_string(),
                tags: vec![Tag {
                    key: "counter_id".to_string(),
                    value: "counter-1".to_string(),
                }],
            },
            metadata: None,
            timestamp: 1000,
        }];

        runner.process_events(&events1).await.unwrap();

        // Second batch includes the same event plus a new one
        let events2 = vec![
            EventRecord {
                position: 1,
                event_id: "evt-1".to_string(),
                event: DomainEvent {
                    event_type: "CounterIncremented".to_string(),
                    data: "{}".to_string(),
                    tags: vec![Tag {
                        key: "counter_id".to_string(),
                        value: "counter-1".to_string(),
                    }],
                },
                metadata: None,
                timestamp: 1000,
            },
            EventRecord {
                position: 2,
                event_id: "evt-2".to_string(),
                event: DomainEvent {
                    event_type: "CounterIncremented".to_string(),
                    data: "{}".to_string(),
                    tags: vec![Tag {
                        key: "counter_id".to_string(),
                        value: "counter-1".to_string(),
                    }],
                },
                metadata: None,
                timestamp: 2000,
            },
        ];

        runner.process_events(&events2).await.unwrap();

        // Should have count of 2, not 3 (event at position 1 was skipped)
        let state = runner.store.get("counter-1").await.unwrap();
        assert_eq!(state.unwrap().count, 2);
    }

    #[tokio::test]
    async fn test_projection_runner_filters_by_event_type() {
        let storage = InMemoryStorage::new();
        let store: StorageBackendProjectionStore<InMemoryStorage, CounterState> =
            StorageBackendProjectionStore::new(
                InMemoryStorage::new(),
                "CounterProjection".to_string(),
            );

        let runner = ProjectionRunner::new(storage, CounterProjection, store);

        let events = vec![
            EventRecord {
                position: 1,
                event_id: "evt-1".to_string(),
                event: DomainEvent {
                    event_type: "CounterIncremented".to_string(),
                    data: "{}".to_string(),
                    tags: vec![Tag {
                        key: "counter_id".to_string(),
                        value: "counter-1".to_string(),
                    }],
                },
                metadata: None,
                timestamp: 1000,
            },
            EventRecord {
                position: 2,
                event_id: "evt-2".to_string(),
                event: DomainEvent {
                    event_type: "SomeOtherEvent".to_string(), // Different event type
                    data: "{}".to_string(),
                    tags: vec![Tag {
                        key: "counter_id".to_string(),
                        value: "counter-1".to_string(),
                    }],
                },
                metadata: None,
                timestamp: 2000,
            },
            EventRecord {
                position: 3,
                event_id: "evt-3".to_string(),
                event: DomainEvent {
                    event_type: "CounterIncremented".to_string(),
                    data: "{}".to_string(),
                    tags: vec![Tag {
                        key: "counter_id".to_string(),
                        value: "counter-1".to_string(),
                    }],
                },
                metadata: None,
                timestamp: 3000,
            },
        ];

        runner.process_events(&events).await.unwrap();

        // Should have count of 2 (only CounterIncremented events)
        let state = runner.store.get("counter-1").await.unwrap();
        assert_eq!(state.unwrap().count, 2);
    }

    #[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
    struct OrderSummary {
        order_id: String,
        customer_name: String,
        total_amount: f64,
        item_count: i32,
    }

    struct TestOrderProjection;

    impl ProjectionDefinition for TestOrderProjection {
        type State = OrderSummary;

        fn projection_name(&self) -> &str {
            "OrderSummary"
        }

        fn event_types(&self) -> Query {
            Query {
                items: vec![crate::domain::QueryItem {
                    event_types: vec![
                        "OrderCreated".to_string(),
                        "ItemAdded".to_string(),
                        "OrderCancelled".to_string(),
                    ],
                    tags: vec![],
                }],
            }
        }

        fn key_selector(&self, event: &EventRecord) -> Option<String> {
            event
                .event
                .tags
                .iter()
                .find(|t| t.key == "orderId")
                .map(|t| t.value.clone())
        }

        fn apply(&self, state: Option<Self::State>, event: &EventRecord) -> Option<Self::State> {
            match event.event.event_type.as_str() {
                "OrderCreated" => Some(OrderSummary {
                    order_id: self.key_selector(event).unwrap_or_default(),
                    customer_name: event.event.data.clone(),
                    total_amount: 0.0,
                    item_count: 0,
                }),
                "ItemAdded" => {
                    if let Some(mut current) = state {
                        let price: f64 = event.event.data.parse().unwrap_or(0.0);
                        current.total_amount += price;
                        current.item_count += 1;
                        Some(current)
                    } else {
                        None
                    }
                }
                "OrderCancelled" => None,
                _ => state,
            }
        }
    }

    fn create_order_event(event_type: &str, data: &str, order_id: &str) -> EventRecord {
        EventRecord {
            position: 1,
            event_id: "evt-1".to_string(),
            event: DomainEvent {
                event_type: event_type.to_string(),
                data: data.to_string(),
                tags: vec![Tag {
                    key: "orderId".to_string(),
                    value: order_id.to_string(),
                }],
            },
            metadata: None,
            timestamp: 1000,
        }
    }

    #[test]
    fn test_order_projection_definition_with_basic_events_applies_correctly() {
        let projection = TestOrderProjection;
        assert_eq!(projection.projection_name(), "OrderSummary");
        let query = projection.event_types();
        assert_eq!(query.items[0].event_types.len(), 3);
        assert!(
            query.items[0]
                .event_types
                .contains(&"OrderCreated".to_string())
        );
        assert!(
            query.items[0]
                .event_types
                .contains(&"ItemAdded".to_string())
        );
        assert!(
            query.items[0]
                .event_types
                .contains(&"OrderCancelled".to_string())
        );
    }

    #[test]
    fn test_order_projection_key_selector_with_valid_event_extracts_key() {
        let projection = TestOrderProjection;
        let evt = create_order_event("OrderCreated", "Customer A", "order-123");
        assert_eq!(projection.key_selector(&evt), Some("order-123".to_string()));
    }

    #[test]
    fn test_order_projection_apply_with_order_created_creates_new_projection() {
        let projection = TestOrderProjection;
        let evt = create_order_event("OrderCreated", "Customer A", "order-123");
        let result = projection.apply(None, &evt).unwrap();
        assert_eq!(result.order_id, "order-123");
        assert_eq!(result.customer_name, "Customer A");
        assert_eq!(result.total_amount, 0.0);
        assert_eq!(result.item_count, 0);
    }

    #[test]
    fn test_order_projection_apply_with_item_added_updates_projection() {
        let projection = TestOrderProjection;
        let current = OrderSummary {
            order_id: "order-123".to_string(),
            customer_name: "Customer A".to_string(),
            total_amount: 100.0,
            item_count: 1,
        };
        let evt = create_order_event("ItemAdded", "50.0", "order-123");
        let result = projection.apply(Some(current), &evt).unwrap();
        assert_eq!(result.total_amount, 150.0);
        assert_eq!(result.item_count, 2);
    }

    #[test]
    fn test_order_projection_apply_with_order_cancelled_returns_none() {
        let projection = TestOrderProjection;
        let current = OrderSummary {
            order_id: "order-123".to_string(),
            customer_name: "Customer A".to_string(),
            total_amount: 100.0,
            item_count: 1,
        };
        let evt = create_order_event("OrderCancelled", "{}", "order-123");
        let result = projection.apply(Some(current), &evt);
        assert!(result.is_none());
    }

    #[test]
    fn test_order_projection_apply_with_unknown_event_returns_current_state() {
        let projection = TestOrderProjection;
        let current = OrderSummary {
            order_id: "order-123".to_string(),
            customer_name: "Customer A".to_string(),
            total_amount: 100.0,
            item_count: 1,
        };
        let evt = create_order_event("UnknownEvent", "{}", "order-123");
        let result = projection.apply(Some(current.clone()), &evt).unwrap();
        assert_eq!(result, current);
    }

    #[test]
    fn test_order_projection_apply_with_item_added_when_current_is_none_returns_none() {
        let projection = TestOrderProjection;
        let evt = create_order_event("ItemAdded", "50.0", "order-123");
        let result = projection.apply(None, &evt);
        assert!(result.is_none());
    }
}
pub mod rebuilder;
pub use rebuilder::*;
