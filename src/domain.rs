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

    fn make_event(event_type: &str, tags: &[(&str, &str)]) -> EventRecord {
        EventRecord {
            position: 1,
            event_id: String::from("id"),
            event: DomainEvent {
                event_type: String::from(event_type),
                data: String::from("{}"),
                tags: tags
                    .iter()
                    .map(|(k, v)| Tag {
                        key: String::from(*k),
                        value: String::from(*v),
                    })
                    .collect(),
            },
            metadata: None,
            timestamp: 0,
        }
    }

    #[test]
    fn matches_query_all_matches_every_event() {
        let query = Query::all();
        let evt = make_event("AnyType", &[("anyKey", "anyValue")]);
        assert!(query.matches(&evt));
    }

    #[test]
    fn matches_query_all_matches_event_with_no_tags() {
        let query = Query::all();
        let evt = make_event("AnyType", &[]);
        assert!(query.matches(&evt));
    }

    #[test]
    fn matches_exact_event_type_returns_true() {
        let query = Query {
            items: alloc::vec![QueryItem {
                event_types: alloc::vec![String::from("StudentRegistered")],
                tags: alloc::vec![],
            }],
        };
        let evt = make_event("StudentRegistered", &[]);
        assert!(query.matches(&evt));
    }

    #[test]
    fn matches_different_event_type_returns_false() {
        let query = Query {
            items: alloc::vec![QueryItem {
                event_types: alloc::vec![String::from("StudentRegistered")],
                tags: alloc::vec![],
            }],
        };
        let evt = make_event("CourseCreated", &[]);
        assert!(!query.matches(&evt));
    }

    #[test]
    fn matches_multiple_types_in_query_item_matches_any() {
        let query = Query {
            items: alloc::vec![QueryItem {
                event_types: alloc::vec![String::from("StudentRegistered"), String::from("CourseCreated")],
                tags: alloc::vec![],
            }],
        };

        assert!(query.matches(&make_event("StudentRegistered", &[])));
        assert!(query.matches(&make_event("CourseCreated", &[])));
        assert!(!query.matches(&make_event("SomethingElse", &[])));
    }

    #[test]
    fn matches_empty_event_types_list_matches_any_type() {
        let query = Query {
            items: alloc::vec![QueryItem {
                event_types: alloc::vec![],
                tags: alloc::vec![],
            }],
        };
        let evt = make_event("AnythingAtAll", &[]);
        assert!(query.matches(&evt));
    }

    #[test]
    fn matches_single_tag_match_returns_true() {
        let query = Query {
            items: alloc::vec![QueryItem {
                event_types: alloc::vec![],
                tags: alloc::vec![Tag {
                    key: String::from("courseId"),
                    value: String::from("abc"),
                }],
            }],
        };
        let evt = make_event("CourseCreated", &[("courseId", "abc")]);
        assert!(query.matches(&evt));
    }

    #[test]
    fn matches_single_tag_key_mismatch_returns_false() {
        let query = Query {
            items: alloc::vec![QueryItem {
                event_types: alloc::vec![],
                tags: alloc::vec![Tag {
                    key: String::from("courseId"),
                    value: String::from("abc"),
                }],
            }],
        };
        let evt = make_event("CourseCreated", &[("studentId", "abc")]);
        assert!(!query.matches(&evt));
    }

    #[test]
    fn matches_single_tag_value_mismatch_returns_false() {
        let query = Query {
            items: alloc::vec![QueryItem {
                event_types: alloc::vec![],
                tags: alloc::vec![Tag {
                    key: String::from("courseId"),
                    value: String::from("abc"),
                }],
            }],
        };
        let evt = make_event("CourseCreated", &[("courseId", "xyz")]);
        assert!(!query.matches(&evt));
    }

    #[test]
    fn matches_all_tags_present_returns_true() {
        let query = Query {
            items: alloc::vec![QueryItem {
                event_types: alloc::vec![],
                tags: alloc::vec![
                    Tag {
                        key: String::from("courseId"),
                        value: String::from("c1"),
                    },
                    Tag {
                        key: String::from("studentId"),
                        value: String::from("s1"),
                    },
                ],
            }],
        };
        let evt = make_event("Enrolled", &[("courseId", "c1"), ("studentId", "s1")]);
        assert!(query.matches(&evt));
    }

    #[test]
    fn matches_one_tag_missing_returns_false() {
        let query = Query {
            items: alloc::vec![QueryItem {
                event_types: alloc::vec![],
                tags: alloc::vec![
                    Tag {
                        key: String::from("courseId"),
                        value: String::from("c1"),
                    },
                    Tag {
                        key: String::from("studentId"),
                        value: String::from("s1"),
                    },
                ],
            }],
        };
        let evt = make_event("Enrolled", &[("courseId", "c1")]);
        assert!(!query.matches(&evt));
    }

    #[test]
    fn matches_event_has_extra_unrelated_tags_still_matches_required() {
        let query = Query {
            items: alloc::vec![QueryItem {
                event_types: alloc::vec![],
                tags: alloc::vec![Tag {
                    key: String::from("courseId"),
                    value: String::from("c1"),
                }],
            }],
        };
        let evt = make_event("CourseCreated", &[("courseId", "c1"), ("region", "EU"), ("tenantId", "t1")]);
        assert!(query.matches(&evt));
    }

    #[test]
    fn matches_correct_type_and_tag_returns_true() {
        let query = Query {
            items: alloc::vec![QueryItem {
                event_types: alloc::vec![String::from("StudentEnrolled")],
                tags: alloc::vec![Tag {
                    key: String::from("courseId"),
                    value: String::from("c1"),
                }],
            }],
        };
        let evt = make_event("StudentEnrolled", &[("courseId", "c1")]);
        assert!(query.matches(&evt));
    }

    #[test]
    fn matches_correct_type_but_wrong_tag_returns_false() {
        let query = Query {
            items: alloc::vec![QueryItem {
                event_types: alloc::vec![String::from("StudentEnrolled")],
                tags: alloc::vec![Tag {
                    key: String::from("courseId"),
                    value: String::from("c1"),
                }],
            }],
        };
        let evt = make_event("StudentEnrolled", &[("courseId", "c2")]);
        assert!(!query.matches(&evt));
    }

    #[test]
    fn matches_second_query_item_matches_returns_true() {
        let query = Query {
            items: alloc::vec![
                QueryItem {
                    event_types: alloc::vec![String::from("StudentRegistered")],
                    tags: alloc::vec![],
                },
                QueryItem {
                    event_types: alloc::vec![String::from("CourseCreated")],
                    tags: alloc::vec![],
                },
            ],
        };
        assert!(query.matches(&make_event("StudentRegistered", &[])));
        assert!(query.matches(&make_event("CourseCreated", &[])));
    }

    #[test]
    fn matches_neither_query_item_matches_returns_false() {
        let query = Query {
            items: alloc::vec![
                QueryItem {
                    event_types: alloc::vec![String::from("StudentRegistered")],
                    tags: alloc::vec![],
                },
                QueryItem {
                    event_types: alloc::vec![String::from("CourseCreated")],
                    tags: alloc::vec![],
                },
            ],
        };
        assert!(!query.matches(&make_event("PaymentProcessed", &[])));
    }

    #[test]
    fn matches_complex_multi_item_query_correctly_evaluates() {
        let query = Query {
            items: alloc::vec![
                QueryItem {
                    event_types: alloc::vec![String::from("StudentEnrolled")],
                    tags: alloc::vec![Tag {
                        key: String::from("courseId"),
                        value: String::from("c1"),
                    }],
                },
                QueryItem {
                    event_types: alloc::vec![String::from("StudentRegistered")],
                    tags: alloc::vec![Tag {
                        key: String::from("studentId"),
                        value: String::from("s1"),
                    }],
                },
            ],
        };

        // Matches item 1
        assert!(query.matches(&make_event("StudentEnrolled", &[("courseId", "c1")])));
        // Matches item 2
        assert!(query.matches(&make_event("StudentRegistered", &[("studentId", "s1")])));
        // Matches neither
        assert!(!query.matches(&make_event("StudentEnrolled", &[("courseId", "c2")])));
        assert!(!query.matches(&make_event("StudentRegistered", &[("studentId", "s2")])));
    }

    #[test]
    fn serialize_with_valid_event_returns_json_string() {
        let evt = make_event("TestEvent", &[]);
        let json_bytes = serde_json_core::to_vec::<_, 1024>(&evt).unwrap();
        let json_str = core::str::from_utf8(&json_bytes).unwrap();
        
        assert!(!json_str.is_empty());
        assert!(json_str.contains("position"));
        assert!(json_str.contains("event"));
    }

    #[test]
    fn serialize_includes_position() {
        let mut evt = make_event("TestEvent", &[]);
        evt.position = 42;
        let json_bytes = serde_json_core::to_vec::<_, 1024>(&evt).unwrap();
        let json_str = core::str::from_utf8(&json_bytes).unwrap();
        
        assert!(json_str.contains("\"position\":"));
        assert!(json_str.contains("42"));
    }

    #[test]
    fn serialize_includes_event_type() {
        let evt = make_event("StudentEnrolled", &[]);
        let json_bytes = serde_json_core::to_vec::<_, 1024>(&evt).unwrap();
        let json_str = core::str::from_utf8(&json_bytes).unwrap();
        
        assert!(json_str.contains("StudentEnrolled"));
    }

    #[test]
    fn serialize_includes_tags() {
        let evt = make_event("TestEvent", &[("courseId", "123"), ("studentId", "456")]);
        let json_bytes = serde_json_core::to_vec::<_, 1024>(&evt).unwrap();
        let json_str = core::str::from_utf8(&json_bytes).unwrap();
        
        assert!(json_str.contains("courseId"));
        assert!(json_str.contains("123"));
        assert!(json_str.contains("studentId"));
        assert!(json_str.contains("456"));
    }

    #[test]
    fn serialize_includes_metadata() {
        let mut evt = make_event("TestEvent", &[]);
        evt.metadata = Some(String::from("correlationId:123"));
        let json_bytes = serde_json_core::to_vec::<_, 1024>(&evt).unwrap();
        let json_str = core::str::from_utf8(&json_bytes).unwrap();
        
        assert!(json_str.contains("metadata"));
        assert!(json_str.contains("correlationId:123"));
    }

    #[test]
    fn deserialize_with_valid_json_returns_record() {
        let evt = make_event("TestEvent", &[]);
        let json_bytes = serde_json_core::to_vec::<_, 1024>(&evt).unwrap();
        let (deserialized, _): (EventRecord, _) = serde_json_core::from_slice(&json_bytes).unwrap();
        
        assert_eq!(evt.position, deserialized.position);
        assert_eq!(evt.event.event_type, deserialized.event.event_type);
    }

    #[test]
    fn deserialize_with_invalid_json_fails() {
        let invalid_json = b"{ this is not valid JSON }";
        let res: Result<(EventRecord, usize), _> = serde_json_core::from_slice(invalid_json);
        assert!(res.is_err());
    }

    #[test]
    fn deserialize_restores_position() {
        let mut evt = make_event("TestEvent", &[]);
        evt.position = 42;
        let json_bytes = serde_json_core::to_vec::<_, 1024>(&evt).unwrap();
        let (deserialized, _): (EventRecord, _) = serde_json_core::from_slice(&json_bytes).unwrap();
        
        assert_eq!(42, deserialized.position);
    }

    #[test]
    fn deserialize_restores_event_type() {
        let evt = make_event("StudentEnrolled", &[]);
        let json_bytes = serde_json_core::to_vec::<_, 1024>(&evt).unwrap();
        let (deserialized, _): (EventRecord, _) = serde_json_core::from_slice(&json_bytes).unwrap();
        
        assert_eq!("StudentEnrolled", deserialized.event.event_type);
    }

    #[test]
    fn deserialize_restores_tags() {
        let evt = make_event("TestEvent", &[("courseId", "123"), ("studentId", "456")]);
        let json_bytes = serde_json_core::to_vec::<_, 1024>(&evt).unwrap();
        let (deserialized, _): (EventRecord, _) = serde_json_core::from_slice(&json_bytes).unwrap();
        
        assert_eq!(2, deserialized.event.tags.len());
        assert!(deserialized.event.tags.iter().any(|t| t.key == "courseId" && t.value == "123"));
        assert!(deserialized.event.tags.iter().any(|t| t.key == "studentId" && t.value == "456"));
    }

    #[test]
    fn deserialize_restores_metadata() {
        let mut evt = make_event("TestEvent", &[]);
        evt.metadata = Some(String::from("correlationId:12345"));
        let json_bytes = serde_json_core::to_vec::<_, 1024>(&evt).unwrap();
        let (deserialized, _): (EventRecord, _) = serde_json_core::from_slice(&json_bytes).unwrap();
        
        assert_eq!(Some(String::from("correlationId:12345")), deserialized.metadata);
    }

    #[test]
    fn round_trip_preserves_all_data() {
        let mut evt = make_event("CompleteEvent", &[("tag1", "value1"), ("tag2", "value2")]);
        evt.position = 123;
        evt.event.data = String::from("complete_data");
        evt.metadata = Some(String::from("meta_data"));
        evt.timestamp = 999999;
        
        let json_bytes = serde_json_core::to_vec::<_, 1024>(&evt).unwrap();
        let (deserialized, _): (EventRecord, _) = serde_json_core::from_slice(&json_bytes).unwrap();
        
        assert_eq!(evt.position, deserialized.position);
        assert_eq!(evt.event.event_type, deserialized.event.event_type);
        assert_eq!(evt.event.tags.len(), deserialized.event.tags.len());
        assert_eq!(evt.metadata, deserialized.metadata);
        assert_eq!(evt.event.data, deserialized.event.data);
        assert_eq!(evt.timestamp, deserialized.timestamp);
    }
}
