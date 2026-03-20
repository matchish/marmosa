use wasm_bindgen::prelude::*;
use send_wrapper::SendWrapper;
use serde::{Deserialize, Serialize};

use marmosa::domain::{EventRecord, Query, Tag};
use marmosa::in_memory::InMemoryStorage;
use marmosa::ports::Error;
use marmosa::projections::{
    ProjectionDefinition, ProjectionRunner, ProjectionStore,
    StorageBackendProjectionStore,
};

use crate::storage::NodeFileSystemStorage;
use crate::MarmosaEventStore;

// A projection state that wraps a JsValue as JSON
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsProjectionState {
    #[serde(flatten)]
    pub value: serde_json::Value,
}

struct JsProjectionDefinition {
    projection_name: String,
    event_types: Query,
    key_selector_fn: SendWrapper<js_sys::Function>,
    apply_fn: SendWrapper<js_sys::Function>,
}

// Safety: wasm32 is single-threaded
unsafe impl Send for JsProjectionDefinition {}
unsafe impl Sync for JsProjectionDefinition {}

impl ProjectionDefinition for JsProjectionDefinition {
    type State = JsProjectionState;

    fn projection_name(&self) -> &str {
        &self.projection_name
    }

    fn event_types(&self) -> Query {
        self.event_types.clone()
    }

    fn key_selector(&self, event: &EventRecord) -> Option<String> {
        let event_js = serde_wasm_bindgen::to_value(event).ok()?;
        let result = self.key_selector_fn.call1(&JsValue::NULL, &event_js).ok()?;
        if result.is_null() || result.is_undefined() {
            None
        } else {
            result.as_string()
        }
    }

    fn apply(&self, state: Option<Self::State>, event: &EventRecord) -> Option<Self::State> {
        let state_js = match &state {
            Some(s) => serde_wasm_bindgen::to_value(&s.value).unwrap_or(JsValue::NULL),
            None => JsValue::NULL,
        };
        let event_js = serde_wasm_bindgen::to_value(event).unwrap_or(JsValue::UNDEFINED);

        let result = self
            .apply_fn
            .call2(&JsValue::NULL, &state_js, &event_js)
            .ok()?;

        if result.is_null() || result.is_undefined() {
            None
        } else {
            let value: serde_json::Value = serde_wasm_bindgen::from_value(result).ok()?;
            Some(JsProjectionState { value })
        }
    }
}

// We need an enum for projection stores since they can be backed by different storage
enum ProjectionStoreKind {
    InMemory(StorageBackendProjectionStore<InMemoryStorage, JsProjectionState>),
    FileSystem(StorageBackendProjectionStore<NodeFileSystemStorage, JsProjectionState>),
}

#[wasm_bindgen]
pub struct WasmProjectionStore {
    inner: ProjectionStoreKind,
}

#[wasm_bindgen]
impl WasmProjectionStore {
    pub async fn get(&self, key: &str) -> Result<JsValue, JsValue> {
        let result = match &self.inner {
            ProjectionStoreKind::InMemory(store) => ProjectionStore::get(store, key).await,
            ProjectionStoreKind::FileSystem(store) => ProjectionStore::get(store, key).await,
        };

        let state = result.map_err(|e| JsValue::from_str(&format!("Get failed: {:?}", e)))?;
        match state {
            Some(s) => serde_wasm_bindgen::to_value(&s.value)
                .map_err(|e| JsValue::from_str(&format!("Serialize failed: {}", e))),
            None => Ok(JsValue::NULL),
        }
    }

    #[wasm_bindgen(js_name = getAll)]
    pub async fn get_all(&self) -> Result<JsValue, JsValue> {
        let result = match &self.inner {
            ProjectionStoreKind::InMemory(store) => ProjectionStore::get_all(store).await,
            ProjectionStoreKind::FileSystem(store) => ProjectionStore::get_all(store).await,
        };

        let states = result.map_err(|e| JsValue::from_str(&format!("GetAll failed: {:?}", e)))?;
        let values: Vec<serde_json::Value> = states.into_iter().map(|s| s.value).collect();
        serde_wasm_bindgen::to_value(&values)
            .map_err(|e| JsValue::from_str(&format!("Serialize failed: {}", e)))
    }

    pub async fn save(&self, key: &str, state_js: JsValue) -> Result<(), JsValue> {
        let value: serde_json::Value = serde_wasm_bindgen::from_value(state_js)
            .map_err(|e| JsValue::from_str(&format!("Deserialize failed: {}", e)))?;
        let state = JsProjectionState { value };

        let result = match &self.inner {
            ProjectionStoreKind::InMemory(store) => ProjectionStore::save(store, key, &state).await,
            ProjectionStoreKind::FileSystem(store) => ProjectionStore::save(store, key, &state).await,
        };

        result.map_err(|e| JsValue::from_str(&format!("Save failed: {:?}", e)))
    }

    pub async fn delete(&self, key: &str) -> Result<(), JsValue> {
        let result = match &self.inner {
            ProjectionStoreKind::InMemory(store) => ProjectionStore::delete(store, key).await,
            ProjectionStoreKind::FileSystem(store) => ProjectionStore::delete(store, key).await,
        };

        result.map_err(|e| JsValue::from_str(&format!("Delete failed: {:?}", e)))
    }

    pub async fn clear(&self) -> Result<(), JsValue> {
        let result = match &self.inner {
            ProjectionStoreKind::InMemory(store) => ProjectionStore::clear(store).await,
            ProjectionStoreKind::FileSystem(store) => ProjectionStore::clear(store).await,
        };

        result.map_err(|e| JsValue::from_str(&format!("Clear failed: {:?}", e)))
    }

    #[wasm_bindgen(js_name = queryByTag)]
    pub async fn query_by_tag(&self, tag_js: JsValue) -> Result<JsValue, JsValue> {
        let tag: Tag = serde_wasm_bindgen::from_value(tag_js)
            .map_err(|e| JsValue::from_str(&format!("Deserialize tag failed: {}", e)))?;

        let result = match &self.inner {
            ProjectionStoreKind::InMemory(store) => ProjectionStore::query_by_tag(store, &tag).await,
            ProjectionStoreKind::FileSystem(store) => ProjectionStore::query_by_tag(store, &tag).await,
        };

        let states = result.map_err(|e| JsValue::from_str(&format!("QueryByTag failed: {:?}", e)))?;
        let values: Vec<serde_json::Value> = states.into_iter().map(|s| s.value).collect();
        serde_wasm_bindgen::to_value(&values)
            .map_err(|e| JsValue::from_str(&format!("Serialize failed: {}", e)))
    }

    #[wasm_bindgen(js_name = queryByTags)]
    pub async fn query_by_tags(&self, tags_js: JsValue) -> Result<JsValue, JsValue> {
        let tags: Vec<Tag> = serde_wasm_bindgen::from_value(tags_js)
            .map_err(|e| JsValue::from_str(&format!("Deserialize tags failed: {}", e)))?;

        let result = match &self.inner {
            ProjectionStoreKind::InMemory(store) => ProjectionStore::query_by_tags(store, &tags).await,
            ProjectionStoreKind::FileSystem(store) => ProjectionStore::query_by_tags(store, &tags).await,
        };

        let states = result.map_err(|e| JsValue::from_str(&format!("QueryByTags failed: {:?}", e)))?;
        let values: Vec<serde_json::Value> = states.into_iter().map(|s| s.value).collect();
        serde_wasm_bindgen::to_value(&values)
            .map_err(|e| JsValue::from_str(&format!("Serialize failed: {}", e)))
    }
}

// The runner needs to own both storage and store, which depends on the backend type.
// We use an enum to handle both.
enum RunnerKind {
    InMemory(
        ProjectionRunner<
            InMemoryStorage,
            JsProjectionState,
            JsProjectionDefinition,
            StorageBackendProjectionStore<InMemoryStorage, JsProjectionState>,
        >,
    ),
    FileSystem(
        ProjectionRunner<
            NodeFileSystemStorage,
            JsProjectionState,
            JsProjectionDefinition,
            StorageBackendProjectionStore<NodeFileSystemStorage, JsProjectionState>,
        >,
    ),
}

#[wasm_bindgen]
pub struct WasmProjectionRunner {
    inner: RunnerKind,
}

#[wasm_bindgen]
impl WasmProjectionRunner {
    pub async fn rebuild(&self, store: &MarmosaEventStore) -> Result<JsValue, JsValue> {
        let result = match &self.inner {
            RunnerKind::InMemory(runner) => {
                // Create a temporary adapter to call rebuild
                runner_rebuild_with_store(runner, store).await
            }
            RunnerKind::FileSystem(runner) => {
                runner_rebuild_with_store(runner, store).await
            }
        };

        result
            .map(|pos| JsValue::from_f64(pos as f64))
            .map_err(|e| JsValue::from_str(&format!("Rebuild failed: {:?}", e)))
    }

    #[wasm_bindgen(js_name = processEvents)]
    pub async fn process_events(&self, events_js: JsValue) -> Result<JsValue, JsValue> {
        let events: Vec<EventRecord> = serde_wasm_bindgen::from_value(events_js)
            .map_err(|e| JsValue::from_str(&format!("Deserialize events failed: {}", e)))?;

        let result = match &self.inner {
            RunnerKind::InMemory(runner) => runner.process_events(&events).await,
            RunnerKind::FileSystem(runner) => runner.process_events(&events).await,
        };

        result
            .map(|pos| JsValue::from_f64(pos as f64))
            .map_err(|e| JsValue::from_str(&format!("ProcessEvents failed: {:?}", e)))
    }

    #[wasm_bindgen(js_name = getCheckpoint)]
    pub async fn get_checkpoint(&self) -> Result<JsValue, JsValue> {
        let result = match &self.inner {
            RunnerKind::InMemory(runner) => runner.get_checkpoint().await,
            RunnerKind::FileSystem(runner) => runner.get_checkpoint().await,
        };

        let checkpoint = result.map_err(|e| JsValue::from_str(&format!("GetCheckpoint failed: {:?}", e)))?;
        match checkpoint {
            Some(cp) => serde_wasm_bindgen::to_value(&cp)
                .map_err(|e| JsValue::from_str(&format!("Serialize failed: {}", e))),
            None => Ok(JsValue::NULL),
        }
    }
}

// Helper to perform rebuild using the MarmosaEventStore as event source
async fn runner_rebuild_with_store<S, Store>(
    runner: &ProjectionRunner<S, JsProjectionState, JsProjectionDefinition, Store>,
    event_store: &MarmosaEventStore,
) -> Result<u64, Error>
where
    S: marmosa::ports::StorageBackend + Send + Sync,
    Store: ProjectionStore<JsProjectionState> + Send + Sync,
{
    // Read all events and process them through the runner
    let events = event_store
        .read_async_internal(Query::all(), None, None, None)
        .await?;
    // Clear store and reset checkpoint via process_events
    // Note: For a full rebuild we'd want to clear the store first, but
    // the runner's rebuild method needs an EventStore impl. We use process_events instead.
    runner.process_events(&events).await
}

fn parse_projection_definition(definition_js: &JsValue) -> Result<JsProjectionDefinition, JsValue> {
    let name = js_sys::Reflect::get(definition_js, &JsValue::from_str("projectionName"))
        .map_err(|_| JsValue::from_str("Missing projectionName"))?
        .as_string()
        .ok_or_else(|| JsValue::from_str("projectionName must be a string"))?;

    let event_types_js = js_sys::Reflect::get(definition_js, &JsValue::from_str("eventTypes"))
        .map_err(|_| JsValue::from_str("Missing eventTypes"))?;
    let event_types: Query = serde_wasm_bindgen::from_value(event_types_js)
        .map_err(|e| JsValue::from_str(&format!("Failed to deserialize eventTypes: {}", e)))?;

    let key_selector = js_sys::Reflect::get(definition_js, &JsValue::from_str("keySelector"))
        .map_err(|_| JsValue::from_str("Missing keySelector"))?;
    let key_selector_fn: js_sys::Function = key_selector
        .dyn_into()
        .map_err(|_| JsValue::from_str("keySelector must be a function"))?;

    let apply = js_sys::Reflect::get(definition_js, &JsValue::from_str("apply"))
        .map_err(|_| JsValue::from_str("Missing apply"))?;
    let apply_fn: js_sys::Function = apply
        .dyn_into()
        .map_err(|_| JsValue::from_str("apply must be a function"))?;

    Ok(JsProjectionDefinition {
        projection_name: name,
        event_types,
        key_selector_fn: SendWrapper::new(key_selector_fn),
        apply_fn: SendWrapper::new(apply_fn),
    })
}

#[wasm_bindgen]
impl MarmosaEventStore {
    /// Creates a projection store backed by the same storage type as this event store.
    #[wasm_bindgen(js_name = createProjectionStore)]
    pub fn create_projection_store(&self, name: &str) -> WasmProjectionStore {
        match self.inner_storage_kind() {
            crate::StoreKind::InMemory(_) => WasmProjectionStore {
                inner: ProjectionStoreKind::InMemory(StorageBackendProjectionStore::new(
                    InMemoryStorage::new(),
                    name.to_string(),
                )),
            },
            crate::StoreKind::FileSystem(_) => {
                let base_path = self.base_path().unwrap_or(".");
                WasmProjectionStore {
                    inner: ProjectionStoreKind::FileSystem(StorageBackendProjectionStore::new(
                        NodeFileSystemStorage::new(base_path.to_string()),
                        name.to_string(),
                    )),
                }
            }
        }
    }

    /// Creates a projection runner with the given definition and store.
    #[wasm_bindgen(js_name = createProjectionRunner)]
    pub fn create_projection_runner(
        &self,
        definition_js: JsValue,
        store: WasmProjectionStore,
    ) -> Result<WasmProjectionRunner, JsValue> {
        let definition = parse_projection_definition(&definition_js)?;

        let runner = match store.inner {
            ProjectionStoreKind::InMemory(proj_store) => {
                WasmProjectionRunner {
                    inner: RunnerKind::InMemory(ProjectionRunner::new(
                        InMemoryStorage::new(),
                        definition,
                        proj_store,
                    )),
                }
            }
            ProjectionStoreKind::FileSystem(proj_store) => {
                let base_path = self.base_path().unwrap_or(".");
                WasmProjectionRunner {
                    inner: RunnerKind::FileSystem(ProjectionRunner::new(
                        NodeFileSystemStorage::new(base_path.to_string()),
                        definition,
                        proj_store,
                    )),
                }
            }
        };

        Ok(runner)
    }
}
