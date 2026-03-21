//! Given/When/Then test harness for [`Decision`] implementations.
//!
//! Tests are sync — no event store, no async runtime. The harness folds
//! given events into state via the decision's projection, calls `process`,
//! and asserts on the result.
//!
//! # Example
//!
//! ```rust
//! use marmosa::testing::TestHarness;
//! use marmosa::decision_model::{Decision, DecisionProjection, DelegateDecisionProjection};
//! use marmosa::domain::{EventData, EventRecord, DomainEvent, Query, QueryItem, Tag};
//!
//! #[derive(Debug, PartialEq)]
//! enum MyError { NotAllowed }
//!
//! struct MyDecision;
//!
//! impl Decision for MyDecision {
//!     type State = DelegateDecisionProjection<bool, fn(bool, &EventRecord) -> bool>;
//!     type Error = MyError;
//!
//!     fn state(&self) -> Self::State {
//!         DelegateDecisionProjection::new(false, Query::all(), |_, _| true)
//!     }
//!
//!     fn process(&self, state: &bool) -> Result<Vec<EventData>, MyError> {
//!         if !state {
//!             return Err(MyError::NotAllowed);
//!         }
//!         Ok(vec![EventData {
//!             event_id: "e1".to_string(),
//!             event: DomainEvent { event_type: "Done".to_string(), data: "{}".to_string(), tags: vec![] },
//!             metadata: None,
//!         }])
//!     }
//! }
//!
//! // given some prior event → when MyDecision → then expect "Done"
//! TestHarness::given(vec![EventData {
//!     event_id: "setup".to_string(),
//!     event: DomainEvent { event_type: "Setup".to_string(), data: "{}".to_string(), tags: vec![] },
//!     metadata: None,
//! }])
//! .when(MyDecision)
//! .then(vec![EventData {
//!     event_id: "e1".to_string(),
//!     event: DomainEvent { event_type: "Done".to_string(), data: "{}".to_string(), tags: vec![] },
//!     metadata: None,
//! }]);
//! ```

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Debug;

use crate::decision_model::{build_decision_model_from_events, Decision, DecisionProjection};
use crate::domain::{DomainEvent, EventData, EventRecord};

/// Converts `EventData` into `EventRecord` with sequential positions and a fixed timestamp.
fn to_event_records(events: &[EventData]) -> Vec<EventRecord> {
    events
        .iter()
        .enumerate()
        .map(|(i, ed)| EventRecord {
            position: (i + 1) as u64,
            event_id: ed.event_id.clone(),
            event: ed.event.clone(),
            metadata: ed.metadata.clone(),
            timestamp: 0,
        })
        .collect()
}

/// Entry point for the test harness.
pub struct TestHarness;

/// Marker for the "given" step.
pub struct Given;

/// Marker for the "when" step, carrying the decision result.
pub struct When<ERR> {
    result: Result<Vec<EventData>, ERR>,
}

/// A step in the test harness pipeline.
pub struct TestHarnessStep<ST> {
    history: Vec<EventData>,
    _step: ST,
}

impl TestHarness {
    /// Sets up a history of events that represent the state before the decision.
    pub fn given(history: impl Into<Vec<EventData>>) -> TestHarnessStep<Given> {
        TestHarnessStep {
            history: history.into(),
            _step: Given,
        }
    }
}

impl TestHarnessStep<Given> {
    /// Executes a decision against the state derived from the given history.
    pub fn when<D>(self, decision: D) -> TestHarnessStep<When<D::Error>>
    where
        D: Decision,
    {
        let projection = decision.state();
        let records = to_event_records(&self.history);
        let model = build_decision_model_from_events(&projection, &records);
        let result = decision.process(&model.state);
        TestHarnessStep {
            history: self.history,
            _step: When { result },
        }
    }
}

impl<ERR: Debug + PartialEq> TestHarnessStep<When<ERR>> {
    /// Asserts that the decision produced the expected events.
    #[track_caller]
    pub fn then(self, expected: impl Into<Vec<EventData>>) {
        assert_eq!(Ok(expected.into()), self._step.result);
    }

    /// Asserts using a custom closure on the produced events.
    #[track_caller]
    pub fn then_assert(self, assertion: impl FnOnce(&Vec<EventData>)) {
        assertion(&self._step.result.unwrap());
    }

    /// Asserts that the decision failed with the expected error.
    #[track_caller]
    pub fn then_err(self, expected: ERR) {
        let err = self._step.result.unwrap_err();
        assert_eq!(err, expected);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decision_model::DelegateDecisionProjection;
    use crate::domain::{DomainEvent, Query, QueryItem, Tag};
    use alloc::string::ToString;
    use alloc::vec;

    // --- Test helpers ---

    fn make_event_data(event_id: &str, event_type: &str, data: &str, tags: Vec<Tag>) -> EventData {
        EventData {
            event_id: event_id.to_string(),
            event: DomainEvent {
                event_type: event_type.to_string(),
                data: data.to_string(),
                tags,
            },
            metadata: None,
        }
    }

    fn course_created(course_id: &str, max_capacity: u32) -> EventData {
        make_event_data(
            &format!("evt-cc-{}", course_id),
            "CourseCreated",
            &format!("{{\"maxCapacity\":{}}}", max_capacity),
            vec![Tag {
                key: "courseId".to_string(),
                value: course_id.to_string(),
            }],
        )
    }

    fn student_enrolled(course_id: &str, student_id: &str) -> EventData {
        make_event_data(
            &format!("evt-se-{}-{}", course_id, student_id),
            "StudentEnrolled",
            &format!("{{\"studentId\":\"{}\"}}", student_id),
            vec![Tag {
                key: "courseId".to_string(),
                value: course_id.to_string(),
            }],
        )
    }

    // --- Decision state ---

    #[derive(Debug, Clone, PartialEq)]
    struct EnrollmentState {
        exists: bool,
        enrolled: u32,
        capacity: u32,
    }

    #[derive(Debug, Clone, PartialEq)]
    enum EnrollError {
        CourseNotFound,
        CourseFull,
    }

    struct EnrollStudent {
        course_id: String,
        student_id: String,
    }

    impl Decision for EnrollStudent {
        type State = DelegateDecisionProjection<EnrollmentState, fn(EnrollmentState, &EventRecord) -> EnrollmentState>;
        type Error = EnrollError;

        fn state(&self) -> Self::State {
            let course_id = self.course_id.clone();
            // We can't use fn pointer with captures, so use a closure-erased form
            // For test purposes, we match on event_type only
            DelegateDecisionProjection::new(
                EnrollmentState {
                    exists: false,
                    enrolled: 0,
                    capacity: 0,
                },
                Query {
                    items: vec![QueryItem {
                        event_types: vec![
                            "CourseCreated".to_string(),
                            "StudentEnrolled".to_string(),
                        ],
                        tags: vec![Tag {
                            key: "courseId".to_string(),
                            value: course_id,
                        }],
                    }],
                },
                |state, evt| match evt.event.event_type.as_str() {
                    "CourseCreated" => {
                        let cap: u32 = serde_json::from_str::<serde_json::Value>(&evt.event.data)
                            .ok()
                            .and_then(|v| v["maxCapacity"].as_u64())
                            .unwrap_or(0) as u32;
                        EnrollmentState {
                            exists: true,
                            capacity: cap,
                            ..state
                        }
                    }
                    "StudentEnrolled" => EnrollmentState {
                        enrolled: state.enrolled + 1,
                        ..state
                    },
                    _ => state,
                },
            )
        }

        fn process(
            &self,
            state: &EnrollmentState,
        ) -> Result<Vec<EventData>, Self::Error> {
            if !state.exists {
                return Err(EnrollError::CourseNotFound);
            }
            if state.enrolled >= state.capacity {
                return Err(EnrollError::CourseFull);
            }
            Ok(vec![student_enrolled(&self.course_id, &self.student_id)])
        }
    }

    // --- Tests ---

    #[test]
    fn given_existing_course_when_enroll_then_produces_enrolled_event() {
        TestHarness::given(vec![course_created("c1", 30)])
            .when(EnrollStudent {
                course_id: "c1".to_string(),
                student_id: "s1".to_string(),
            })
            .then(vec![student_enrolled("c1", "s1")]);
    }

    #[test]
    fn given_full_course_when_enroll_then_err_course_full() {
        TestHarness::given(vec![
            course_created("c1", 1),
            student_enrolled("c1", "s-other"),
        ])
        .when(EnrollStudent {
            course_id: "c1".to_string(),
            student_id: "s1".to_string(),
        })
        .then_err(EnrollError::CourseFull);
    }

    #[test]
    fn given_no_course_when_enroll_then_err_course_not_found() {
        TestHarness::given(vec![])
            .when(EnrollStudent {
                course_id: "c1".to_string(),
                student_id: "s1".to_string(),
            })
            .then_err(EnrollError::CourseNotFound);
    }

    #[test]
    fn then_assert_allows_custom_assertions() {
        TestHarness::given(vec![course_created("c1", 30)])
            .when(EnrollStudent {
                course_id: "c1".to_string(),
                student_id: "s1".to_string(),
            })
            .then_assert(|events| {
                assert_eq!(events.len(), 1);
                assert_eq!(events[0].event.event_type, "StudentEnrolled");
            });
    }

    #[test]
    #[should_panic]
    fn then_panics_when_error_but_events_expected() {
        TestHarness::given(vec![])
            .when(EnrollStudent {
                course_id: "c1".to_string(),
                student_id: "s1".to_string(),
            })
            .then(vec![student_enrolled("c1", "s1")]);
    }

    #[test]
    #[should_panic]
    fn then_err_panics_when_success_but_error_expected() {
        TestHarness::given(vec![course_created("c1", 30)])
            .when(EnrollStudent {
                course_id: "c1".to_string(),
                student_id: "s1".to_string(),
            })
            .then_err(EnrollError::CourseFull);
    }
}
