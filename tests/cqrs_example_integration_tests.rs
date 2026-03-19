mod common;

use common::{FakeClock, InMemoryStorage};
use std::sync::Arc;
use serde::{Deserialize, Serialize};

use marmosa::domain::{DomainEvent, EventData, EventRecord, Query, Tag, QueryItem};
use marmosa::event_store::{EventStore, OpossumStore};
use marmosa::decision_model::{DecisionModelExt, DecisionProjection};

#[derive(Serialize, Deserialize, Clone)]
struct CourseCreated {
    course_id: String,
    max_capacity: i32,
}

#[derive(Serialize, Deserialize, Clone)]
struct CourseCapacityUpdatedEvent {
    course_id: String,
    new_capacity: i32,
}

#[derive(Serialize, Deserialize, Clone)]
struct StudentEnrolledToCourseEvent {
    course_id: String,
    student_id: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct StudentUnenrolledFromCourseEvent {
    course_id: String,
    student_id: String,
}

#[derive(Clone, Default)]
struct CourseEnlistmentAggregate {
    course_max_capacity: i32,
    course_current_enrollment_count: i32,
    student_current_course_enrollment_count: i32,
    student_max_course_enrollment_limit: i32,
    is_student_already_enrolled: bool,
}

impl CourseEnlistmentAggregate {
    fn can_enroll_student(&self) -> bool {
        self.course_current_enrollment_count < self.course_max_capacity &&
        self.student_current_course_enrollment_count < self.student_max_course_enrollment_limit
    }

    fn can_unenroll_student(&self) -> bool {
        self.course_current_enrollment_count > 0 && self.is_student_already_enrolled
    }

    fn get_enrollment_failure_reason(&self) -> Option<String> {
        if self.course_current_enrollment_count >= self.course_max_capacity {
            Some("Course is at maximum capacity".to_string())
        } else if self.student_current_course_enrollment_count >= self.student_max_course_enrollment_limit {
            Some(format!("Student has reached maximum course enrollment limit ({})", self.student_max_course_enrollment_limit))
        } else {
            None
        }
    }
}

struct EnrollmentDecisionModel {
    course_id: String,
    student_id: String,
    query: Query,
}

impl EnrollmentDecisionModel {
    fn new(course_id: String, student_id: String) -> Self {
        let query = Query {
            items: vec![
                QueryItem {
                    event_types: vec![
                        "StudentEnrolledToCourseEvent".to_string(),
                        "StudentUnenrolledFromCourseEvent".to_string(),
                        "CourseCreated".to_string(),
                        "CourseCapacityUpdatedEvent".to_string(),
                    ],
                    tags: vec![Tag {
                        key: "courseId".to_string(),
                        value: course_id.clone()
                    }],
                },
                QueryItem {
                    event_types: vec![
                        "StudentEnrolledToCourseEvent".to_string(),
                        "StudentUnenrolledFromCourseEvent".to_string(),
                    ],
                    tags: vec![Tag {
                        key: "studentId".to_string(),
                        value: student_id.clone()
                    }],
                }
            ]
        };
        Self { course_id, student_id, query }
    }
}

impl DecisionProjection for EnrollmentDecisionModel {
    type State = CourseEnlistmentAggregate;

    fn initial_state(&self) -> Self::State {
        CourseEnlistmentAggregate {
            course_max_capacity: 0,
            course_current_enrollment_count: 0,
            student_current_course_enrollment_count: 0,
            student_max_course_enrollment_limit: 3,
            is_student_already_enrolled: false,
        }
    }

    fn query(&self) -> &Query {
        &self.query
    }

    fn apply(&self, state: Self::State, event: &EventRecord) -> Self::State {
        let mut new_state = state.clone();
        match event.event.event_type.as_str() {
            "CourseCreated" => {
                if let Ok(data) = serde_json::from_str::<CourseCreated>(&event.event.data) {
                    if data.course_id == self.course_id {
                        new_state.course_max_capacity = data.max_capacity;
                    }
                }
            }
            "CourseCapacityUpdatedEvent" => {
                if let Ok(data) = serde_json::from_str::<CourseCapacityUpdatedEvent>(&event.event.data) {
                    if data.course_id == self.course_id {
                        new_state.course_max_capacity = data.new_capacity;
                    }
                }
            }
            "StudentEnrolledToCourseEvent" => {
                if let Ok(data) = serde_json::from_str::<StudentEnrolledToCourseEvent>(&event.event.data) {
                    if data.course_id == self.course_id {
                        new_state.course_current_enrollment_count += 1;
                    }
                    if data.student_id == self.student_id {
                        new_state.student_current_course_enrollment_count += 1;
                    }
                    if data.course_id == self.course_id && data.student_id == self.student_id {
                        new_state.is_student_already_enrolled = true;
                    }
                }
            }
            "StudentUnenrolledFromCourseEvent" => {
                if let Ok(data) = serde_json::from_str::<StudentUnenrolledFromCourseEvent>(&event.event.data) {
                    if data.course_id == self.course_id {
                        new_state.course_current_enrollment_count = std::cmp::max(0, new_state.course_current_enrollment_count - 1);
                    }
                    if data.student_id == self.student_id {
                        new_state.student_current_course_enrollment_count = std::cmp::max(0, new_state.student_current_course_enrollment_count - 1);
                    }
                    if data.course_id == self.course_id && data.student_id == self.student_id {
                        new_state.is_student_already_enrolled = false;
                    }
                }
            }
            _ => {}
        }
        new_state
    }
}

// Handlers
struct CommandResult {
    success: bool,
    error: Option<String>,
}

async fn handle_create_course(store: &(impl EventStore + Send + Sync), course_id: String, max_capacity: i32) -> CommandResult {
    let ev = CourseCreated { course_id: course_id.clone(), max_capacity };
    let data = EventData {
        event_id: uuid::Uuid::new_v4().to_string(),
        event: DomainEvent {
            event_type: "CourseCreated".to_string(),
            data: serde_json::to_string(&ev).unwrap(),
            tags: vec![Tag { key: "courseId".to_string(), value: course_id }],
        },
        metadata: None,
    };
    store.append_async(vec![data], None).await.unwrap();
    CommandResult { success: true, error: None }
}

async fn handle_enroll_student(store: &(impl EventStore + Send + Sync), course_id: String, student_id: String) -> CommandResult {
    let model = store.build_decision_model_async(EnrollmentDecisionModel::new(course_id.clone(), student_id.clone())).await.unwrap();
    if model.state.course_max_capacity == 0 {
        return CommandResult { success: false, error: Some("Course not found".to_string()) };
    }
    if model.state.is_student_already_enrolled {
        return CommandResult { success: false, error: Some("Student already enrolled".to_string()) };
    }
    if !model.state.can_enroll_student() {
        return CommandResult { success: false, error: model.state.get_enrollment_failure_reason() };
    }
    let ev = StudentEnrolledToCourseEvent { course_id: course_id.clone(), student_id: student_id.clone() };
    let data = EventData {
        event_id: uuid::Uuid::new_v4().to_string(),
        event: DomainEvent {
            event_type: "StudentEnrolledToCourseEvent".to_string(),
            data: serde_json::to_string(&ev).unwrap(),
            tags: vec![
                Tag { key: "courseId".to_string(), value: course_id.clone() },
                Tag { key: "studentId".to_string(), value: student_id.clone() },
            ],
        },
        metadata: None,
    };
    store.append_async(vec![data], Some(model.append_condition)).await.unwrap();
    CommandResult { success: true, error: None }
}

async fn handle_unenroll_student(store: &(impl EventStore + Send + Sync), course_id: String, student_id: String) -> CommandResult {
    let model = store.build_decision_model_async(EnrollmentDecisionModel::new(course_id.clone(), student_id.clone())).await.unwrap();
    if !model.state.can_unenroll_student() {
        return CommandResult { success: false, error: Some("Cannot unenroll".to_string()) };
    }
    let ev = StudentUnenrolledFromCourseEvent { course_id: course_id.clone(), student_id: student_id.clone() };
    let data = EventData {
        event_id: uuid::Uuid::new_v4().to_string(),
        event: DomainEvent {
            event_type: "StudentUnenrolledFromCourseEvent".to_string(),
            data: serde_json::to_string(&ev).unwrap(),
            tags: vec![
                Tag { key: "courseId".to_string(), value: course_id.clone() },
                Tag { key: "studentId".to_string(), value: student_id.clone() },
            ],
        },
        metadata: None,
    };
    store.append_async(vec![data], Some(model.append_condition)).await.unwrap();
    CommandResult { success: true, error: None }
}

// Tests

fn create_store() -> impl EventStore {
    let storage = Arc::new(InMemoryStorage::new());
    OpossumStore::new(storage, FakeClock::new(100))
}

#[tokio::test]
async fn enroll_student_to_course_should_create_event_and_build_aggregate_async() {
    let store = create_store();
    let course_id = uuid::Uuid::new_v4().to_string();
    let student_id = uuid::Uuid::new_v4().to_string();

    handle_create_course(&store, course_id.clone(), 30).await;
    let res = handle_enroll_student(&store, course_id.clone(), student_id.clone()).await;
    assert!(res.success);

    let model = store.build_decision_model_async(EnrollmentDecisionModel::new(course_id, student_id)).await.unwrap();
    assert_eq!(model.state.course_current_enrollment_count, 1);
    assert_eq!(model.state.student_current_course_enrollment_count, 1);
}

#[tokio::test]
async fn enroll_student_when_course_full_should_fail_async() {
    let store = create_store();
    let course_id = uuid::Uuid::new_v4().to_string();

    handle_create_course(&store, course_id.clone(), 1).await;
    
    let res = handle_enroll_student(&store, course_id.clone(), uuid::Uuid::new_v4().to_string()).await;
    assert!(res.success);

    let res2 = handle_enroll_student(&store, course_id.clone(), uuid::Uuid::new_v4().to_string()).await;
    assert!(!res2.success);
    assert_eq!(res2.error, Some("Course is at maximum capacity".to_string()));
}

#[tokio::test]
async fn enroll_student_when_student_at_max_capacity_should_fail_async() {
    let store = create_store();
    let student_id = uuid::Uuid::new_v4().to_string();

    for _ in 0..3 {
        let course_id = uuid::Uuid::new_v4().to_string();
        handle_create_course(&store, course_id.clone(), 10).await;
        let res = handle_enroll_student(&store, course_id.clone(), student_id.clone()).await;
        assert!(res.success);
    }

    let course_id4 = uuid::Uuid::new_v4().to_string();
    handle_create_course(&store, course_id4.clone(), 10).await;
    let res4 = handle_enroll_student(&store, course_id4.clone(), student_id.clone()).await;
    assert!(!res4.success);
    assert!(res4.error.unwrap().contains("Student has reached maximum"));
}

#[tokio::test]
async fn unenroll_student_should_decrease_counts_async() {
    let store = create_store();
    let course_id = uuid::Uuid::new_v4().to_string();
    let student_id = uuid::Uuid::new_v4().to_string();

    handle_create_course(&store, course_id.clone(), 30).await;
    handle_enroll_student(&store, course_id.clone(), student_id.clone()).await;
    
    let un_res = handle_unenroll_student(&store, course_id.clone(), student_id.clone()).await;
    assert!(un_res.success);

    let model = store.build_decision_model_async(EnrollmentDecisionModel::new(course_id, student_id)).await.unwrap();
    assert_eq!(model.state.course_current_enrollment_count, 0);
    assert_eq!(model.state.student_current_course_enrollment_count, 0);
    assert_eq!(model.state.is_student_already_enrolled, false);
}
