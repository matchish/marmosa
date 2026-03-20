use wasm_bindgen::prelude::*;
use send_wrapper::SendWrapper;

use marmosa::decision_model::{
    build_decision_model_from_events, DecisionProjection,
};
use marmosa::domain::{EventRecord, Query};

use crate::MarmosaEventStore;

struct JsDecisionProjection {
    initial_state: SendWrapper<JsValue>,
    query: Query,
    apply_fn: SendWrapper<js_sys::Function>,
}

// Safety: wasm32 is single-threaded, SendWrapper ensures these are only accessed on the main thread
unsafe impl Send for JsDecisionProjection {}
unsafe impl Sync for JsDecisionProjection {}

impl DecisionProjection for JsDecisionProjection {
    type State = JsValue;

    fn initial_state(&self) -> Self::State {
        (*self.initial_state).clone()
    }

    fn query(&self) -> &Query {
        &self.query
    }

    fn apply(&self, state: Self::State, event: &EventRecord) -> Self::State {
        let event_js = serde_wasm_bindgen::to_value(event).unwrap_or(JsValue::UNDEFINED);
        self.apply_fn
            .call2(&JsValue::NULL, &state, &event_js)
            .unwrap_or(state)
    }
}

#[wasm_bindgen]
impl MarmosaEventStore {
    /// Builds a decision model from a JS projection definition.
    ///
    /// projection: { initialState: T, query: Query, apply: (state: T, event: EventRecord) => T }
    #[wasm_bindgen(js_name = buildDecisionModel)]
    pub async fn build_decision_model(
        &self,
        projection_js: JsValue,
    ) -> Result<JsValue, JsValue> {
        let initial_state = js_sys::Reflect::get(&projection_js, &JsValue::from_str("initialState"))
            .map_err(|_| JsValue::from_str("Missing initialState"))?;
        let query_js = js_sys::Reflect::get(&projection_js, &JsValue::from_str("query"))
            .map_err(|_| JsValue::from_str("Missing query"))?;
        let apply_fn = js_sys::Reflect::get(&projection_js, &JsValue::from_str("apply"))
            .map_err(|_| JsValue::from_str("Missing apply"))?;

        let query: Query = serde_wasm_bindgen::from_value(query_js)
            .map_err(|e| JsValue::from_str(&format!("Failed to deserialize query: {}", e)))?;
        let apply: js_sys::Function = apply_fn
            .dyn_into()
            .map_err(|_| JsValue::from_str("apply must be a function"))?;

        let projection = JsDecisionProjection {
            initial_state: SendWrapper::new(initial_state),
            query: query.clone(),
            apply_fn: SendWrapper::new(apply),
        };

        // Read events matching the projection query
        let events = self
            .read_async_internal(query, None, None, None)
            .await
            .map_err(|e| JsValue::from_str(&format!("Failed to read events: {:?}", e)))?;

        let model = build_decision_model_from_events(&projection, &events);

        // Return { state, appendCondition }
        let result = js_sys::Object::new();
        js_sys::Reflect::set(&result, &JsValue::from_str("state"), &model.state)
            .map_err(|_| JsValue::from_str("Failed to set state"))?;
        let condition_js = serde_wasm_bindgen::to_value(&model.append_condition)
            .map_err(|e| JsValue::from_str(&format!("Failed to serialize condition: {}", e)))?;
        js_sys::Reflect::set(&result, &JsValue::from_str("appendCondition"), &condition_js)
            .map_err(|_| JsValue::from_str("Failed to set appendCondition"))?;

        Ok(result.into())
    }

    /// Executes a decision with retry logic.
    ///
    /// operation: async (store: MarmosaEventStore) => R
    #[wasm_bindgen(js_name = executeDecision)]
    pub async fn execute_decision(
        &self,
        max_retries: u32,
        operation: js_sys::Function,
    ) -> Result<JsValue, JsValue> {
        let mut attempt = 0u32;
        loop {
            let this_js: JsValue = JsValue::UNDEFINED;
            // We pass `this` as the store reference — the JS side already has a reference
            let promise = operation
                .call1(&this_js, &JsValue::from(self as *const _ as u32))
                .map_err(|e| JsValue::from_str(&format!("Operation call failed: {:?}", e)))?;

            // If the operation returns a promise, await it
            let result = if promise.has_type::<js_sys::Promise>() {
                wasm_bindgen_futures::JsFuture::from(js_sys::Promise::from(promise)).await
            } else {
                Ok(promise)
            };

            match result {
                Ok(value) => return Ok(value),
                Err(err) => {
                    let is_condition_failed = js_sys::Reflect::get(&err, &JsValue::from_str("message"))
                        .ok()
                        .and_then(|v| v.as_string())
                        .map(|msg| msg.contains("AppendConditionFailed"))
                        .unwrap_or(false);

                    if is_condition_failed && attempt < max_retries - 1 {
                        attempt += 1;
                        continue;
                    }
                    return Err(err);
                }
            }
        }
    }
}
