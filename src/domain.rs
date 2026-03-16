use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Tag {
    pub key: String,
    pub value: String,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct DomainEvent {
    pub event_type: String,
    pub data: String, // Stringified JSON
    pub tags: Vec<Tag>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct EventData {
    pub event_id: String,
    pub event: DomainEvent,
    pub metadata: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct EventRecord {
    pub position: u64,
    pub event_id: String,
    pub event: DomainEvent,
    pub metadata: Option<String>,
    pub timestamp: u64,
}

#[derive(Debug, PartialEq, Clone)]
pub struct QueryItem {
    pub event_types: Vec<String>,
    pub tags: Vec<Tag>,
}

#[derive(Debug, PartialEq, Clone)]
pub struct Query {
    pub items: Vec<QueryItem>,
}

impl Query {
    pub fn all() -> Self {
        Self { items: Vec::new() }
    }

    pub fn matches(&self, record: &EventRecord) -> bool {
        if self.items.is_empty() {
            return true;
        }

        for item in &self.items {
            let type_match = item.event_types.is_empty() 
                || item.event_types.iter().any(|t| t == &record.event.event_type);

            let tag_match = item.tags.is_empty() 
                || item.tags.iter().all(|t| {
                    record.event.tags.iter().any(|rt| rt.key == t.key && rt.value == t.value)
                });

            if type_match && tag_match {
                return true;
            }
        }
        false
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct AppendCondition {
    pub fail_if_events_match: Query,
    pub after_sequence_position: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json_core::from_slice;

    #[test]
    fn test_event_data_deserialization_no_alloc() {
        let json = br#"{"event_id":"id1","event":{"event_type":"type1","data":"{}","tags":[]},"metadata":null}"#;
        let (parsed, _): (EventData, _) = from_slice(json).unwrap();
        assert_eq!(parsed.event_id, "id1");
        assert_eq!(parsed.event.event_type, "type1");
    }
}
