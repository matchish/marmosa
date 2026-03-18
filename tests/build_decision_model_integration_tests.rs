mod common;

use common::{FakeClock, InMemoryStorage};
use marmosa::{
    decision_model::{DecisionModelExt, DecisionProjection},
    domain::{DomainEvent, EventData, EventRecord, Query, QueryItem, Tag},
    event_store::{EventStore, OpossumStore},
};
use std::sync::Arc;

// Minimal event types for tests
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct CourseCreatedEvent {
    course_id: String,
    max_students: i32,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct StudentEnrolledEvent {
    course_id: String,
    student_id: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct UnrelatedEvent {
    data: String,
}

// Helper projections
struct CourseExistsProjection {
    query: Query,
}

impl CourseExistsProjection {
    fn new(course_id: impl Into<String>) -> Self {
        let query = Query {
            items: vec![QueryItem {
                event_types: vec!["CourseCreatedEvent".to_string()],
                tags: vec![Tag {
                    key: "course_id".to_string(),
                    value: course_id.into(),
                }],
            }],
        };
        Self { query }
    }
}

impl DecisionProjection for CourseExistsProjection {
    type State = bool;

    fn initial_state(&self) -> Self::State {
        false
    }

    fn query(&self) -> &Query {
        &self.query
    }

    fn apply(&self, _state: Self::State, evt: &EventRecord) -> Self::State {
        evt.event.event_type == "CourseCreatedEvent"
    }
}

struct EnrollmentCountProjection {
    query: Query,
}

impl EnrollmentCountProjection {
    fn new(course_id: impl Into<String>) -> Self {
        let query = Query {
            items: vec![QueryItem {
                event_types: vec!["StudentEnrolledEvent".to_string()],
                tags: vec![Tag {
                    key: "course_id".to_string(),
                    value: course_id.into(),
                }],
            }],
        };
        Self { query }
    }
}

impl DecisionProjection for EnrollmentCountProjection {
    type State = i32;

    fn initial_state(&self) -> Self::State {
        0
    }

    fn query(&self) -> &Query {
        &self.query
    }

    fn apply(&self, count: Self::State, evt: &EventRecord) -> Self::State {
        if evt.event.event_type == "StudentEnrolledEvent" {
            count + 1
        } else {
            count
        }
    }
}

fn create_event_store() -> Arc<OpossumStore<Arc<InMemoryStorage>, FakeClock>> {
    let storage = Arc::new(InMemoryStorage::new());
    Arc::new(OpossumStore::new(storage, FakeClock::new(1000)))
}

#[tokio::test]
async fn build_decision_model_async_empty_store_state_is_initial_state_async() {
    let store = create_event_store();
    let course_id = uuid::Uuid::new_v4().to_string();

    let model = store
        .build_decision_model_async(CourseExistsProjection::new(course_id.clone()))
        .await
        .unwrap();

    assert_eq!(model.state, false);
}

#[tokio::test]
async fn build_decision_model_async_empty_store_after_sequence_position_is_null_async() {
    let store = create_event_store();
    let course_id = uuid::Uuid::new_v4().to_string();

    let model = store
        .build_decision_model_async(CourseExistsProjection::new(course_id.clone()))
        .await
        .unwrap();

    assert_eq!(model.append_condition.after_sequence_position, None);
}

#[tokio::test]
async fn build_decision_model_async_empty_store_fail_if_events_match_is_projection_query_async() {
    let store = create_event_store();
    let course_id = uuid::Uuid::new_v4().to_string();
    let projection = CourseExistsProjection::new(course_id.clone());

    let model = store
        .build_decision_model_async(CourseExistsProjection::new(course_id.clone()))
        .await
        .unwrap();

    // The method return type for append condition contains `fail_if_events_match` of type Option<Query>
    assert_eq!(
        model.append_condition.fail_if_events_match,
        projection.query().clone()
    );
}

#[tokio::test]
async fn build_decision_model_async_after_append_state_reflects_events_async() {
    let store = create_event_store();
    let course_id = uuid::Uuid::new_v4().to_string();

    store
        .append_async(
            vec![EventData {
                event_id: uuid::Uuid::new_v4().to_string(),
                metadata: None,
                event: DomainEvent {
                    event_type: "CourseCreatedEvent".to_string(),
                    data: serde_json::to_string(&CourseCreatedEvent {
                        course_id: course_id.clone(),
                        max_students: 10,
                    })
                    .unwrap(),
                    tags: vec![Tag {
                        key: "course_id".to_string(),
                        value: course_id.clone(),
                    }],
                },
            }],
            None,
        )
        .await
        .unwrap();

    let model = store
        .build_decision_model_async(CourseExistsProjection::new(course_id.clone()))
        .await
        .unwrap();

    assert_eq!(model.state, true);
}

#[tokio::test]
async fn build_decision_model_async_after_multiple_enrollments_count_is_correct_async() {
    let store = create_event_store();
    let course_id = uuid::Uuid::new_v4().to_string();

    for i in 0..3 {
        store
            .append_async(
                vec![EventData {
                    event_id: uuid::Uuid::new_v4().to_string(),
                    metadata: None,
                    event: DomainEvent {
                        event_type: "StudentEnrolledEvent".to_string(),
                        data: serde_json::to_string(&StudentEnrolledEvent {
                            course_id: course_id.clone(),
                            student_id: format!("student-{}", i),
                        })
                        .unwrap(),
                        tags: vec![Tag {
                            key: "course_id".to_string(),
                            value: course_id.clone(),
                        }],
                    },
                }],
                None,
            )
            .await
            .unwrap();
    }

    let model = store
        .build_decision_model_async(EnrollmentCountProjection::new(course_id.clone()))
        .await
        .unwrap();

    assert_eq!(model.state, 3);
}

#[tokio::test]
async fn build_decision_model_nary_async_three_projections_all_states_correct_async() {
    let store = create_event_store();
    let course_id = uuid::Uuid::new_v4().to_string();

    store
        .append_async(
            vec![
                EventData {
                    event_id: uuid::Uuid::new_v4().to_string(),
                    metadata: None,
                    event: DomainEvent {
                        event_type: "CourseCreatedEvent".to_string(),
                        data: serde_json::to_string(&CourseCreatedEvent {
                            course_id: course_id.clone(),
                            max_students: 10,
                        })
                        .unwrap(),
                        tags: vec![Tag {
                            key: "course_id".to_string(),
                            value: course_id.clone(),
                        }],
                    },
                },
                EventData {
                    event_id: uuid::Uuid::new_v4().to_string(),
                    metadata: None,
                    event: DomainEvent {
                        event_type: "StudentEnrolledEvent".to_string(),
                        data: serde_json::to_string(&StudentEnrolledEvent {
                            course_id: course_id.clone(),
                            student_id: "student-1".to_string(),
                        })
                        .unwrap(),
                        tags: vec![Tag {
                            key: "course_id".to_string(),
                            value: course_id.clone(),
                        }],
                    },
                },
                EventData {
                    event_id: uuid::Uuid::new_v4().to_string(),
                    metadata: None,
                    event: DomainEvent {
                        event_type: "StudentEnrolledEvent".to_string(),
                        data: serde_json::to_string(&StudentEnrolledEvent {
                            course_id: course_id.clone(),
                            student_id: "student-2".to_string(),
                        })
                        .unwrap(),
                        tags: vec![Tag {
                            key: "course_id".to_string(),
                            value: course_id.clone(),
                        }],
                    },
                },
            ],
            None,
        )
        .await
        .unwrap();

    let p1 = EnrollmentCountProjection::new(course_id.clone());
    let p2 = EnrollmentCountProjection::new(course_id.clone());
    let p3 = EnrollmentCountProjection::new(course_id.clone());
    let projections: Vec<&(dyn DecisionProjection<State = i32> + Send + Sync)> =
        vec![&p1, &p2, &p3];

    let (states, condition) = store
        .build_decision_model_nary_async(&projections)
        .await
        .unwrap();

    assert_eq!(states, vec![2, 2, 2]);
    assert!(condition.after_sequence_position.is_some());
}
