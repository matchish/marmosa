use wasm_bindgen::prelude::*;

pub mod decision;
pub mod projections;
pub mod storage;

use marmosa::domain::{AppendCondition, EventData, EventRecord, Query};
use marmosa::event_store::{EventStore, MarmosaStore};
use marmosa::extensions::EventStoreExt;
use marmosa::in_memory::InMemoryStorage;
use marmosa::ports::Clock;
use storage::NodeFileSystemStorage;

pub struct JsClock;

impl Clock for JsClock {
    fn now_millis(&self) -> u64 {
        js_sys::Date::now() as u64
    }
}

enum StoreKind {
    InMemory(MarmosaStore<InMemoryStorage, JsClock>),
    FileSystem(MarmosaStore<NodeFileSystemStorage, JsClock>),
}

#[wasm_bindgen]
pub struct MarmosaEventStore {
    inner: StoreKind,
    base_path: Option<String>,
}

#[wasm_bindgen]
impl MarmosaEventStore {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: StoreKind::InMemory(MarmosaStore::new(InMemoryStorage::new(), JsClock)),
            base_path: None,
        }
    }

    #[wasm_bindgen(js_name = withFileSystem)]
    pub fn with_file_system(base_path: &str) -> Self {
        Self {
            inner: StoreKind::FileSystem(MarmosaStore::new(
                NodeFileSystemStorage::new(base_path.to_string()),
                JsClock,
            )),
            base_path: Some(base_path.to_string()),
        }
    }

    pub async fn append(
        &self,
        events_js: JsValue,
        condition_js: JsValue,
    ) -> Result<(), JsValue> {
        let events: Vec<EventData> = serde_wasm_bindgen::from_value(events_js)
            .map_err(|e| JsValue::from_str(&format!("Failed to deserialize events: {}", e)))?;

        let condition: Option<AppendCondition> = if condition_js.is_null() || condition_js.is_undefined() {
            None
        } else {
            Some(
                serde_wasm_bindgen::from_value(condition_js)
                    .map_err(|e| JsValue::from_str(&format!("Failed to deserialize condition: {}", e)))?,
            )
        };

        let result = match &self.inner {
            StoreKind::InMemory(store) => store.append_async(events, condition).await,
            StoreKind::FileSystem(store) => store.append_async(events, condition).await,
        };

        result.map_err(|e| JsValue::from_str(&format!("Append failed: {:?}", e)))
    }

    pub async fn read(
        &self,
        query_js: JsValue,
        start_position: JsValue,
        max_count: JsValue,
    ) -> Result<JsValue, JsValue> {
        let query: Query = serde_wasm_bindgen::from_value(query_js)
            .map_err(|e| JsValue::from_str(&format!("Failed to deserialize query: {}", e)))?;

        let start_pos = if start_position.is_null() || start_position.is_undefined() {
            None
        } else {
            start_position.as_f64().map(|v| v as u64)
        };

        let max = if max_count.is_null() || max_count.is_undefined() {
            None
        } else {
            max_count.as_f64().map(|v| v as usize)
        };

        let result = match &self.inner {
            StoreKind::InMemory(store) => store.read_async(query, start_pos, max, None).await,
            StoreKind::FileSystem(store) => store.read_async(query, start_pos, max, None).await,
        };

        let events = result.map_err(|e| JsValue::from_str(&format!("Read failed: {:?}", e)))?;
        serde_wasm_bindgen::to_value(&events)
            .map_err(|e| JsValue::from_str(&format!("Failed to serialize events: {}", e)))
    }

    #[wasm_bindgen(js_name = readAll)]
    pub async fn read_all(&self, query_js: JsValue) -> Result<JsValue, JsValue> {
        let query: Query = serde_wasm_bindgen::from_value(query_js)
            .map_err(|e| JsValue::from_str(&format!("Failed to deserialize query: {}", e)))?;

        let result = match &self.inner {
            StoreKind::InMemory(store) => store.read_all_async(query).await,
            StoreKind::FileSystem(store) => store.read_all_async(query).await,
        };

        let events = result.map_err(|e| JsValue::from_str(&format!("ReadAll failed: {:?}", e)))?;
        serde_wasm_bindgen::to_value(&events)
            .map_err(|e| JsValue::from_str(&format!("Failed to serialize events: {}", e)))
    }

    #[wasm_bindgen(js_name = readLast)]
    pub async fn read_last(&self, query_js: JsValue) -> Result<JsValue, JsValue> {
        let query: Query = serde_wasm_bindgen::from_value(query_js)
            .map_err(|e| JsValue::from_str(&format!("Failed to deserialize query: {}", e)))?;

        let result = match &self.inner {
            StoreKind::InMemory(store) => store.read_last_async(query).await,
            StoreKind::FileSystem(store) => store.read_last_async(query).await,
        };

        let event = result.map_err(|e| JsValue::from_str(&format!("ReadLast failed: {:?}", e)))?;
        serde_wasm_bindgen::to_value(&event)
            .map_err(|e| JsValue::from_str(&format!("Failed to serialize event: {}", e)))
    }
}

// Internal helpers for other modules to access the store
impl MarmosaEventStore {
    pub(crate) async fn read_async_internal(
        &self,
        query: Query,
        start_position: Option<u64>,
        max_count: Option<usize>,
        options: Option<Vec<marmosa::domain::ReadOption>>,
    ) -> Result<Vec<EventRecord>, marmosa::ports::Error> {
        match &self.inner {
            StoreKind::InMemory(store) => {
                store.read_async(query, start_position, max_count, options).await
            }
            StoreKind::FileSystem(store) => {
                store.read_async(query, start_position, max_count, options).await
            }
        }
    }

    #[allow(dead_code)]
    pub(crate) async fn append_async_internal(
        &self,
        events: Vec<EventData>,
        condition: Option<AppendCondition>,
    ) -> Result<(), marmosa::ports::Error> {
        match &self.inner {
            StoreKind::InMemory(store) => store.append_async(events, condition).await,
            StoreKind::FileSystem(store) => store.append_async(events, condition).await,
        }
    }

    pub(crate) fn inner_storage_kind(&self) -> &StoreKind {
        &self.inner
    }

    pub(crate) fn base_path(&self) -> Option<&str> {
        self.base_path.as_deref()
    }
}
