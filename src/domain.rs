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
                || item
                    .event_types
                    .iter()
                    .any(|t| t == &record.event.event_type);

            let tag_match = item.tags.is_empty()
                || item.tags.iter().all(|t| {
                    record
                        .event
                        .tags
                        .iter()
                        .any(|rt| rt.key == t.key && rt.value == t.value)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReadOption(pub u32);

impl ReadOption {
    pub const NONE: ReadOption = ReadOption(0);
    pub const DESCENDING: ReadOption = ReadOption(1);

    pub fn has_flag(&self, flag: ReadOption) -> bool {
        if flag.0 == 0 {
            true // In C#, any enum value HasFlag(0) is true
        } else {
            (self.0 & flag.0) == flag.0
        }
    }
}

impl core::fmt::Display for ReadOption {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.0 {
            0 => write!(f, "None"),
            1 => write!(f, "Descending"),
            _ => write!(f, "{}", self.0),
        }
    }
}

impl core::str::FromStr for ReadOption {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "None" => Ok(ReadOption::NONE),
            "Descending" => Ok(ReadOption::DESCENDING),
            _ => Err(()),
        }
    }
}


#[derive(Debug, Clone, PartialEq)]
pub struct CommandResult {
    pub success: bool,
    pub error_message: Option<String>,
}

impl CommandResult {
    pub fn new(success: bool, error_message: Option<String>) -> Self {
        Self { success, error_message }
    }

    pub fn ok() -> Self {
        Self {
            success: true,
            error_message: None,
        }
    }

    pub fn fail(error_message: impl Into<String>) -> Self {
        Self {
            success: false,
            error_message: Some(error_message.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommandResultWith<T> {
    pub success: bool,
    pub value: Option<T>,
    pub error_message: Option<String>,
}

impl<T> CommandResultWith<T> {
    pub fn new(success: bool, value: Option<T>, error_message: Option<String>) -> Self {
        Self { success, value, error_message }
    }

    pub fn ok(value: T) -> Self {
        Self {
            success: true,
            value: Some(value),
            error_message: None,
        }
    }

    pub fn fail(error_message: impl Into<String>) -> Self {
        Self {
            success: false,
            value: None,
            error_message: Some(error_message.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json_core::from_slice;

    #[test]
    fn read_option_none_has_value_zero() {
        assert_eq!(ReadOption::NONE.0, 0);
    }

    #[test]
    fn read_option_descending_has_value_one() {
        assert_eq!(ReadOption::DESCENDING.0, 1);
    }

    #[test]
    fn read_option_none_is_default_value() {
        let default_value = ReadOption::default();
        assert_eq!(ReadOption::NONE, default_value);
    }

    #[test]
    fn read_option_can_check_for_none() {
        let option = ReadOption::NONE;
        assert!(option == ReadOption::NONE);
        assert!(!option.has_flag(ReadOption::DESCENDING));
    }

    #[test]
    fn read_option_can_check_for_descending() {
        let option = ReadOption::DESCENDING;
        assert!(option.has_flag(ReadOption::DESCENDING));
        assert!(option != ReadOption::NONE);
    }

    #[test]
    fn read_option_has_flag_works_with_none() {
        let option = ReadOption::NONE;
        assert!(option.has_flag(ReadOption::NONE));
    }

    #[test]
    fn read_option_default_parameter_is_none() {
        fn test_method(option: Option<ReadOption>) {
            let option = option.unwrap_or(ReadOption::NONE);
            assert_eq!(ReadOption::NONE, option);
        }
        test_method(None);
    }

    #[test]
    fn read_option_can_be_passed_as_parameter() {
        fn test_method(option: ReadOption) {
            assert_eq!(ReadOption::DESCENDING, option);
        }
        test_method(ReadOption::DESCENDING);
    }

    #[test]
    fn read_option_can_be_used_in_if_statement() {
        let option = ReadOption::DESCENDING;
        let mut was_descending = false;

        if option.has_flag(ReadOption::DESCENDING) {
            was_descending = true;
        }

        assert!(was_descending);
    }

    #[test]
    fn read_option_can_be_used_in_match_statement() {
        let option = ReadOption::DESCENDING;
        let mut result = "";

        match option {
            ReadOption::NONE => result = "ascending",
            ReadOption::DESCENDING => result = "descending",
            _ => {}
        }

        assert_eq!("descending", result);
    }

    #[test]
    fn read_option_supports_value_comparison() {
        let option1 = ReadOption::NONE;
        let option2 = ReadOption::NONE;
        let option3 = ReadOption::DESCENDING;

        assert_eq!(option1, option2);
        assert_ne!(option1, option3);
    }

    #[test]
    fn read_option_can_be_converted_to_int() {
        let none_value = ReadOption::NONE.0;
        let descending_value = ReadOption::DESCENDING.0;

        assert_eq!(0, none_value);
        assert_eq!(1, descending_value);
    }

    #[test]
    fn read_option_can_be_converted_from_int() {
        let none = ReadOption(0);
        let descending = ReadOption(1);

        assert_eq!(ReadOption::NONE, none);
        assert_eq!(ReadOption::DESCENDING, descending);
    }

    #[test]
    fn read_option_has_flag_works_correctly() {
        assert!(!ReadOption::NONE.has_flag(ReadOption::DESCENDING));
        assert!(ReadOption::DESCENDING.has_flag(ReadOption::DESCENDING));
    }

    #[test]
    fn read_option_to_string_returns_name() {
        use alloc::string::ToString;
        let none_string = ReadOption::NONE.to_string();
        let descending_string = ReadOption::DESCENDING.to_string();

        assert_eq!("None", none_string);
        assert_eq!("Descending", descending_string);
    }

    #[test]
    fn read_option_can_parse_from_string() {
        use core::str::FromStr;
        let none = ReadOption::from_str("None").unwrap();
        let descending = ReadOption::from_str("Descending").unwrap();

        assert_eq!(ReadOption::NONE, none);
        assert_eq!(ReadOption::DESCENDING, descending);
    }

    
    #[test]
    fn command_result_constructor_sets_properties_correctly() {
        let result = CommandResult::new(true, Some(String::from("Test error")));
        assert!(result.success);
        assert_eq!(result.error_message, Some(String::from("Test error")));
    }

    #[test]
    fn command_result_ok_creates_successful_result() {
        let result = CommandResult::ok();
        assert!(result.success);
        assert_eq!(result.error_message, None);
    }

    #[test]
    fn command_result_fail_creates_failed_result_with_message() {
        let error_message = "Operation failed";
        let result = CommandResult::fail(error_message);
        assert!(!result.success);
        assert_eq!(result.error_message, Some(String::from(error_message)));
    }

    #[test]
    fn command_result_with_optional_error_message_defaults_to_null() {
        let result = CommandResult::new(true, None);
        assert!(result.success);
        assert_eq!(result.error_message, None);
    }

    #[test]
    fn command_result_is_record_supports_value_equality() {
        let result1 = CommandResult::new(true, Some(String::from("Error")));
        let result2 = CommandResult::new(true, Some(String::from("Error")));
        assert_eq!(result1, result2);
    }

    #[test]
    fn command_result_different_instances_are_not_equal() {
        let result1 = CommandResult::ok();
        let result2 = CommandResult::fail("Error");
        assert_ne!(result1, result2);
    }

    #[derive(Debug, Clone, PartialEq)]
    struct TestData {
        id: i32,
        name: String,
    }

    #[test]
    fn command_result_generic_constructor_sets_properties_correctly() {
        let result = CommandResultWith::new(true, Some(String::from("test")), Some(String::from("Error")));
        assert!(result.success);
        assert_eq!(result.value, Some(String::from("test")));
        assert_eq!(result.error_message, Some(String::from("Error")));
    }

    #[test]
    fn command_result_generic_ok_creates_successful_result_with_value() {
        let result = CommandResultWith::ok(String::from("Test Value"));
        assert!(result.success);
        assert_eq!(result.value, Some(String::from("Test Value")));
        assert_eq!(result.error_message, None);
    }

    #[test]
    fn command_result_generic_fail_creates_failed_result_without_value() {
        let error_message = "Operation failed";
        let result = CommandResultWith::<String>::fail(error_message);
        assert!(!result.success);
        assert_eq!(result.value, None);
        assert_eq!(result.error_message, Some(String::from(error_message)));
    }

    #[test]
    fn command_result_generic_with_complex_type_stores_value_correctly() {
        let test_object = TestData { id: 42, name: String::from("Test") };
        let result = CommandResultWith::ok(test_object.clone());
        assert!(result.success);
        assert_eq!(result.value, Some(test_object));
    }

    #[test]
    fn command_result_generic_with_value_type_works_correctly() {
        let result = CommandResultWith::ok(123);
        assert!(result.success);
        assert_eq!(result.value, Some(123));
    }

    #[test]
    fn command_result_generic_is_record_supports_value_equality() {
        let result1 = CommandResultWith::new(true, Some(42), None);
        let result2 = CommandResultWith::new(true, Some(42), None);
        assert_eq!(result1, result2);
    }

    #[test]
    fn command_result_generic_different_values_are_not_equal() {
        let result1 = CommandResultWith::ok(42);
        let result2 = CommandResultWith::ok(99);
        assert_ne!(result1, result2);
    }

    #[test]
    fn command_result_generic_with_list_works_correctly() {
        let list = alloc::vec![String::from("Item1"), String::from("Item2"), String::from("Item3")];
        let result = CommandResultWith::ok(list);
        assert!(result.success);
        assert_eq!(result.value.as_ref().unwrap().len(), 3);
        assert!(result.value.as_ref().unwrap().contains(&String::from("Item2")));
    }

    #[test]
    fn command_result_generic_failure_with_default_value_has_null_value() {
        let result = CommandResultWith::<String>::fail("Error occurred");
        assert!(!result.success);
        assert_eq!(result.value, None);
        assert_eq!(result.error_message, Some(String::from("Error occurred")));
    }

    #[test]
    fn command_result_typical_usage_pattern_works_as_expected() {
        fn execute_command(should_succeed: bool) -> CommandResult {
            if should_succeed {
                CommandResult::ok()
            } else {
                CommandResult::fail("Command execution failed")
            }
        }

        let success_result = execute_command(true);
        assert!(success_result.success);

        let fail_result = execute_command(false);
        assert!(!fail_result.success);
        assert_eq!(fail_result.error_message, Some(String::from("Command execution failed")));
    }

    #[test]
    fn command_result_generic_typical_usage_pattern_works_as_expected() {
        fn execute_query(should_succeed: bool) -> CommandResultWith<alloc::vec::Vec<String>> {
            if should_succeed {
                CommandResultWith::ok(alloc::vec![String::from("Result1"), String::from("Result2")])
            } else {
                CommandResultWith::fail("Query execution failed")
            }
        }

        let success_result = execute_query(true);
        assert!(success_result.success);
        assert_eq!(success_result.value.as_ref().unwrap().len(), 2);

        let fail_result = execute_query(false);
        assert!(!fail_result.success);
        assert_eq!(fail_result.value, None);
        assert_eq!(fail_result.error_message, Some(String::from("Query execution failed")));
    }

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
                event_types: alloc::vec![
                    String::from("StudentRegistered"),
                    String::from("CourseCreated")
                ],
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
        let evt = make_event(
            "CourseCreated",
            &[("courseId", "c1"), ("region", "EU"), ("tenantId", "t1")],
        );
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
        assert!(
            deserialized
                .event
                .tags
                .iter()
                .any(|t| t.key == "courseId" && t.value == "123")
        );
        assert!(
            deserialized
                .event
                .tags
                .iter()
                .any(|t| t.key == "studentId" && t.value == "456")
        );
    }

    #[test]
    fn deserialize_restores_metadata() {
        let mut evt = make_event("TestEvent", &[]);
        evt.metadata = Some(String::from("correlationId:12345"));
        let json_bytes = serde_json_core::to_vec::<_, 1024>(&evt).unwrap();
        let (deserialized, _): (EventRecord, _) = serde_json_core::from_slice(&json_bytes).unwrap();

        assert_eq!(
            Some(String::from("correlationId:12345")),
            deserialized.metadata
        );
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
