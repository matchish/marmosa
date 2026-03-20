use wasm_bindgen::prelude::*;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

use marmosa_wasm::MarmosaEventStore;

#[wasm_bindgen_test]
async fn test_in_memory_append_and_read_all() {
    let store = MarmosaEventStore::new();

    let events = serde_wasm_bindgen::to_value(&serde_json::json!([{
        "event_id": "e1",
        "event": {
            "event_type": "TestEvent",
            "data": "{}",
            "tags": []
        },
        "metadata": null
    }]))
    .unwrap();

    store.append(events, JsValue::NULL).await.unwrap();

    let query = serde_wasm_bindgen::to_value(&serde_json::json!({ "items": [] })).unwrap();
    let result = store.read_all(query).await.unwrap();

    let events: Vec<serde_json::Value> = serde_wasm_bindgen::from_value(result).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["event_id"], "e1");
    assert_eq!(events[0]["position"], 0);
}

#[wasm_bindgen_test]
async fn test_in_memory_append_multiple_and_read() {
    let store = MarmosaEventStore::new();

    let events = serde_wasm_bindgen::to_value(&serde_json::json!([
        {
            "event_id": "e1",
            "event": { "event_type": "TypeA", "data": "{}", "tags": [] },
            "metadata": null
        },
        {
            "event_id": "e2",
            "event": { "event_type": "TypeB", "data": "{}", "tags": [] },
            "metadata": null
        }
    ]))
    .unwrap();

    store.append(events, JsValue::NULL).await.unwrap();

    let query = serde_wasm_bindgen::to_value(&serde_json::json!({
        "items": [{ "event_types": ["TypeA"], "tags": [] }]
    }))
    .unwrap();

    let result = store.read_all(query).await.unwrap();
    let events: Vec<serde_json::Value> = serde_wasm_bindgen::from_value(result).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["event"]["event_type"], "TypeA");
}

#[wasm_bindgen_test]
async fn test_in_memory_read_last() {
    let store = MarmosaEventStore::new();

    let events = serde_wasm_bindgen::to_value(&serde_json::json!([
        {
            "event_id": "e1",
            "event": { "event_type": "Test", "data": "first", "tags": [] },
            "metadata": null
        },
        {
            "event_id": "e2",
            "event": { "event_type": "Test", "data": "second", "tags": [] },
            "metadata": null
        }
    ]))
    .unwrap();

    store.append(events, JsValue::NULL).await.unwrap();

    let query = serde_wasm_bindgen::to_value(&serde_json::json!({ "items": [] })).unwrap();
    let result = store.read_last(query).await.unwrap();
    let event: serde_json::Value = serde_wasm_bindgen::from_value(result).unwrap();
    assert_eq!(event["event_id"], "e2");
    assert_eq!(event["position"], 1);
}

#[wasm_bindgen_test]
async fn test_append_condition_prevents_duplicate() {
    let store = MarmosaEventStore::new();

    let events1 = serde_wasm_bindgen::to_value(&serde_json::json!([{
        "event_id": "e1",
        "event": { "event_type": "Test", "data": "{}", "tags": [] },
        "metadata": null
    }]))
    .unwrap();

    store.append(events1, JsValue::NULL).await.unwrap();

    let events2 = serde_wasm_bindgen::to_value(&serde_json::json!([{
        "event_id": "e2",
        "event": { "event_type": "Test", "data": "{}", "tags": [] },
        "metadata": null
    }]))
    .unwrap();

    let condition = serde_wasm_bindgen::to_value(&serde_json::json!({
        "fail_if_events_match": { "items": [] },
        "after_sequence_position": null
    }))
    .unwrap();

    let result = store.append(events2, condition).await;
    assert!(result.is_err());
}

#[wasm_bindgen_test]
async fn test_decision_model_build() {
    let store = MarmosaEventStore::new();

    // Append some events
    let events = serde_wasm_bindgen::to_value(&serde_json::json!([
        {
            "event_id": "e1",
            "event": { "event_type": "Increment", "data": "{}", "tags": [] },
            "metadata": null
        },
        {
            "event_id": "e2",
            "event": { "event_type": "Increment", "data": "{}", "tags": [] },
            "metadata": null
        }
    ]))
    .unwrap();

    store.append(events, JsValue::NULL).await.unwrap();

    // Build decision model with a counter projection
    let apply = js_sys::Function::new_with_args(
        "state, event",
        "return event.event_type === 'Increment' ? state + 1 : state;",
    );

    let query = serde_wasm_bindgen::to_value(&serde_json::json!({ "items": [] })).unwrap();

    let projection = js_sys::Object::new();
    js_sys::Reflect::set(&projection, &JsValue::from_str("initialState"), &JsValue::from(0))
        .unwrap();
    js_sys::Reflect::set(&projection, &JsValue::from_str("query"), &query).unwrap();
    js_sys::Reflect::set(&projection, &JsValue::from_str("apply"), &apply).unwrap();

    let result = store
        .build_decision_model(projection.into())
        .await
        .unwrap();

    let state = js_sys::Reflect::get(&result, &JsValue::from_str("state")).unwrap();
    assert_eq!(state.as_f64().unwrap(), 2.0);

    let condition = js_sys::Reflect::get(&result, &JsValue::from_str("appendCondition")).unwrap();
    assert!(!condition.is_undefined());
}
