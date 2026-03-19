mod common;

use common::{FakeClock, InMemoryStorage};
use marmosa::domain::{
    AppendCondition, DomainEvent, EventData, EventRecord, Query, QueryItem, Tag,
};
use marmosa::event_store::{EventStore, MarmosaStore};
use marmosa::ports::Error;
use std::sync::Arc;
use uuid::Uuid;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
struct CourseCreated {
    pub course_id: String,
    pub max_capacity: i32,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
struct StudentEnrolled {
    pub course_id: String,
    pub student_id: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
struct RegisterStudent {
    pub student_id: String,
    pub name: String,
}

#[derive(Debug)]
struct CourseEnlistmentAggregate {
    course_id: String,
    student_id: String,
    course_max_capacity: i32,
    course_current_enrollment_count: i32,
    student_max_course_enrollment_limit: i32,
    student_current_course_enrollment_count: i32,
}

impl CourseEnlistmentAggregate {
    fn new(course_id: String, student_id: String) -> Self {
        Self {
            course_id,
            student_id,
            course_max_capacity: 0,
            course_current_enrollment_count: 0,
            student_max_course_enrollment_limit: 5,
            student_current_course_enrollment_count: 0,
        }
    }

    fn apply(&mut self, event: &EventRecord) {
        let clean_json = event.event.data.replace("\\\"", "\"");
        match event.event.event_type.as_str() {
            "CourseCreated" => {
                if let Ok(data) = serde_json::from_str::<CourseCreated>(&clean_json)
                    && data.course_id == self.course_id
                {
                    self.course_max_capacity = data.max_capacity;
                }
            }
            "StudentEnrolledToCourseEvent" => {
                if let Ok(data) = serde_json::from_str::<StudentEnrolled>(&clean_json) {
                    if data.course_id == self.course_id {
                        self.course_current_enrollment_count += 1;
                    }
                    if data.student_id == self.student_id {
                        self.student_current_course_enrollment_count += 1;
                    }
                }
            }
            "StudentUnenrolledFromCourseEvent" => {
                if let Ok(data) = serde_json::from_str::<StudentEnrolled>(&clean_json) {
                    if data.course_id == self.course_id {
                        self.course_current_enrollment_count = std::cmp::max(
                            0,
                            self.course_current_enrollment_count.saturating_sub(1),
                        );
                    }
                    if data.student_id == self.student_id {
                        self.student_current_course_enrollment_count = std::cmp::max(
                            0,
                            self.student_current_course_enrollment_count
                                .saturating_sub(1),
                        );
                    }
                }
            }
            _ => {}
        }
    }

    fn can_enroll_student(&self) -> bool {
        self.course_current_enrollment_count < self.course_max_capacity
            && self.student_current_course_enrollment_count
                < self.student_max_course_enrollment_limit
    }
}

async fn create_course(
    store: Arc<MarmosaStore<Arc<InMemoryStorage>, FakeClock>>,
    course_id: String,
    max_capacity: i32,
) -> Result<(), Error> {
    let data = serde_json::to_string(&CourseCreated {
        course_id: course_id.clone(),
        max_capacity,
    })
    .unwrap();
    let event = EventData {
        event_id: Uuid::new_v4().to_string(),
        event: DomainEvent {
            event_type: "CourseCreated".to_string(),
            data,
            tags: vec![Tag {
                key: "courseId".to_string(),
                value: course_id,
            }],
        },
        metadata: None,
    };
    store.append_async(vec![event], None).await
}

async fn enroll_student(
    store: Arc<MarmosaStore<Arc<InMemoryStorage>, FakeClock>>,
    course_id: String,
    student_id: String,
) -> Result<bool, Error> {
    let max_retries = 10;

    for attempt in 0..max_retries {
        let query = Query {
            items: vec![
                QueryItem {
                    tags: vec![Tag {
                        key: "courseId".to_string(),
                        value: course_id.clone(),
                    }],
                    event_types: vec![
                        "CourseCreated".to_string(),
                        "StudentEnrolledToCourseEvent".to_string(),
                        "StudentUnenrolledFromCourseEvent".to_string(),
                    ],
                },
                QueryItem {
                    tags: vec![Tag {
                        key: "studentId".to_string(),
                        value: student_id.clone(),
                    }],
                    event_types: vec![
                        "StudentEnrolledToCourseEvent".to_string(),
                        "StudentUnenrolledFromCourseEvent".to_string(),
                    ],
                },
            ],
        };

        let events = store.read_async(query.clone(), None, None, None).await?;
        let last_position = events.last().map(|e| e.position);

        let mut agg = CourseEnlistmentAggregate::new(course_id.clone(), student_id.clone());
        let mut is_already_enrolled = false;

        for e in &events {
            agg.apply(e);
            if e.event.event_type == "StudentEnrolledToCourseEvent" {
                let clean_json = e.event.data.replace("\\\"", "\"");
                if let Ok(data) = serde_json::from_str::<StudentEnrolled>(&clean_json)
                    && data.course_id == course_id
                    && data.student_id == student_id
                {
                    is_already_enrolled = true;
                }
            }
        }

        if is_already_enrolled {
            return Ok(false);
        }

        if !agg.can_enroll_student() {
            return Ok(false);
        }

        let data = serde_json::to_string(&StudentEnrolled {
            course_id: course_id.clone(),
            student_id: student_id.clone(),
        })
        .unwrap();
        let new_event = EventData {
            event_id: Uuid::new_v4().to_string(),
            event: DomainEvent {
                event_type: "StudentEnrolledToCourseEvent".to_string(),
                data,
                tags: vec![
                    Tag {
                        key: "courseId".to_string(),
                        value: course_id.clone(),
                    },
                    Tag {
                        key: "studentId".to_string(),
                        value: student_id.clone(),
                    },
                ],
            },
            metadata: None,
        };

        let condition = AppendCondition {
            after_sequence_position: last_position,
            fail_if_events_match: query,
        };

        match store.append_async(vec![new_event], Some(condition)).await {
            Ok(_) => return Ok(true),
            Err(Error::AppendConditionFailed) => {
                tokio::time::sleep(tokio::time::Duration::from_millis(10 * (attempt + 1))).await;
                continue;
            }
            Err(e) => return Err(e),
        }
    }

    Ok(false)
}

#[tokio::test]
async fn independent_commands_should_execute_concurrently_without_conflict_async() {
    let storage = Arc::new(InMemoryStorage::new());
    let store = Arc::new(MarmosaStore::new(storage.clone(), FakeClock::new(1000)));

    let student_id = Uuid::new_v4().to_string();
    let course_id = Uuid::new_v4().to_string();

    let store1 = store.clone();
    let s_id = student_id.clone();
    let task1 = tokio::spawn(async move {
        let data = serde_json::to_string(&RegisterStudent {
            student_id: s_id.clone(),
            name: "John Doe".to_string(),
        })
        .unwrap();
        let evt = EventData {
            event_id: Uuid::new_v4().to_string(),
            event: DomainEvent {
                event_type: "StudentRegisteredEvent".to_string(),
                data,
                tags: vec![Tag {
                    key: "studentId".to_string(),
                    value: s_id,
                }],
            },
            metadata: None,
        };
        store1.append_async(vec![evt], None).await.unwrap();
    });

    let store2 = store.clone();
    let c_id = course_id.clone();
    let task2 = tokio::spawn(async move {
        create_course(store2, c_id, 30).await.unwrap();
    });

    let _ = tokio::join!(task1, task2);

    let student_events = store
        .read_async(
            Query {
                items: vec![QueryItem {
                    event_types: vec!["StudentRegisteredEvent".to_string()],
                    tags: vec![],
                }],
            },
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert!(!student_events.is_empty());

    let course_events = store
        .read_async(
            Query {
                items: vec![QueryItem {
                    event_types: vec!["CourseCreated".to_string()],
                    tags: vec![],
                }],
            },
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert!(!course_events.is_empty());
}

#[tokio::test]
async fn concurrent_enrollments_when_course_has_one_spot_left_should_allow_only_one_async() {
    let storage = Arc::new(InMemoryStorage::new());
    let store = Arc::new(MarmosaStore::new(storage.clone(), FakeClock::new(1000)));

    let course_id = Uuid::new_v4().to_string();
    create_course(store.clone(), course_id.clone(), 10)
        .await
        .unwrap();

    for _ in 0..9 {
        let student_id = Uuid::new_v4().to_string();
        assert!(
            enroll_student(store.clone(), course_id.clone(), student_id)
                .await
                .unwrap()
        );
    }

    let queries = Query {
        items: vec![QueryItem {
            event_types: vec!["StudentEnrolledToCourseEvent".to_string()],
            tags: vec![Tag {
                key: "courseId".to_string(),
                value: course_id.clone(),
            }],
        }],
    };
    let enrolled_count = store
        .read_async(queries.clone(), None, None, None)
        .await
        .unwrap()
        .len();
    assert_eq!(enrolled_count, 9);

    let student10_id = Uuid::new_v4().to_string();
    let student11_id = Uuid::new_v4().to_string();

    let store1 = store.clone();
    let c1 = course_id.clone();
    let s10 = student10_id.clone();
    let task1 = tokio::spawn(async move { enroll_student(store1, c1, s10).await.unwrap() });

    let store2 = store.clone();
    let c2 = course_id.clone();
    let s11 = student11_id.clone();
    let task2 = tokio::spawn(async move { enroll_student(store2, c2, s11).await.unwrap() });

    let (res1, res2) = tokio::join!(task1, task2);

    let r1 = res1.unwrap();
    let r2 = res2.unwrap();

    let successes = [r1, r2].iter().filter(|&&x| x).count();
    let failures = [r1, r2].iter().filter(|&&x| !x).count();

    assert_eq!(successes, 1);
    assert_eq!(failures, 1);

    let enrolled_count_final = store
        .read_async(queries, None, None, None)
        .await
        .unwrap()
        .len();
    assert_eq!(enrolled_count_final, 10);
}

#[tokio::test]
async fn concurrent_enrollments_to_different_courses_should_all_succeed_async() {
    let storage = Arc::new(InMemoryStorage::new());
    let store = Arc::new(MarmosaStore::new(storage.clone(), FakeClock::new(1000)));

    let student_id = Uuid::new_v4().to_string();
    let courses = vec![
        Uuid::new_v4().to_string(),
        Uuid::new_v4().to_string(),
        Uuid::new_v4().to_string(),
    ];

    for c in &courses {
        create_course(store.clone(), c.clone(), 30).await.unwrap();
    }

    let mut tasks = vec![];
    for c in &courses {
        let store_clone = store.clone();
        let c_clone = c.clone();
        let s_clone = student_id.clone();
        tasks.push(tokio::spawn(async move {
            enroll_student(store_clone, c_clone, s_clone).await.unwrap()
        }));
    }

    for t in tasks {
        assert!(t.await.unwrap());
    }

    let query = Query {
        items: vec![QueryItem {
            tags: vec![Tag {
                key: "studentId".to_string(),
                value: student_id,
            }],
            event_types: vec!["StudentEnrolledToCourseEvent".to_string()],
        }],
    };
    let student_events = store.read_async(query, None, None, None).await.unwrap();
    assert_eq!(student_events.len(), 3);
}

#[tokio::test]
async fn concurrent_enrollments_same_student_same_course_should_only_allow_once_async() {
    let storage = Arc::new(InMemoryStorage::new());
    let store = Arc::new(MarmosaStore::new(storage.clone(), FakeClock::new(1000)));

    let student_id = Uuid::new_v4().to_string();
    let course_id = Uuid::new_v4().to_string();

    create_course(store.clone(), course_id.clone(), 30)
        .await
        .unwrap();

    let store1 = store.clone();
    let c1 = course_id.clone();
    let s1 = student_id.clone();
    let task1 = tokio::spawn(async move { enroll_student(store1, c1, s1).await.unwrap_or(false) });

    let store2 = store.clone();
    let c2 = course_id.clone();
    let s2 = student_id.clone();
    let task2 = tokio::spawn(async move { enroll_student(store2, c2, s2).await.unwrap_or(false) });

    let (res1, res2) = tokio::join!(task1, task2);

    let r1 = res1.unwrap();
    let r2 = res2.unwrap();
    assert!(r1 || r2); // At least one should succeed

    let query = Query {
        items: vec![QueryItem {
            tags: vec![
                Tag {
                    key: "courseId".to_string(),
                    value: course_id,
                },
                Tag {
                    key: "studentId".to_string(),
                    value: student_id,
                },
            ],
            event_types: vec!["StudentEnrolledToCourseEvent".to_string()],
        }],
    };
    let events = store.read_async(query, None, None, None).await.unwrap();
    assert!(events.len() <= 1, "Expected at most 1 enrollment event");
}

#[tokio::test]
async fn concurrent_enrollments_many_students_one_course_should_respect_capacity_async() {
    let storage = Arc::new(InMemoryStorage::new());
    let store = Arc::new(MarmosaStore::new(storage.clone(), FakeClock::new(1000)));

    let course_id = Uuid::new_v4().to_string();
    let capacity = 10;
    let attempt_count = 20;

    create_course(store.clone(), course_id.clone(), capacity)
        .await
        .unwrap();

    let mut tasks = vec![];
    for _ in 0..attempt_count {
        let student_id = Uuid::new_v4().to_string();
        let store_clone = store.clone();
        let c_clone = course_id.clone();
        tasks.push(tokio::spawn(async move {
            enroll_student(store_clone, c_clone, student_id)
                .await
                .unwrap_or(false)
        }));
    }

    let mut successes = 0;
    let mut failures = 0;
    for t in tasks {
        if t.await.unwrap() {
            successes += 1;
        } else {
            failures += 1;
        }
    }

    assert_eq!(successes, capacity);
    assert_eq!(failures, attempt_count - capacity);
}

#[tokio::test]
async fn failed_append_should_release_lock_allowing_subsequent_operations_async() {
    let storage = Arc::new(InMemoryStorage::new());
    let store = Arc::new(MarmosaStore::new(storage.clone(), FakeClock::new(1000)));

    let course_id = Uuid::new_v4().to_string();
    let student1_id = Uuid::new_v4().to_string();

    create_course(store.clone(), course_id.clone(), 1)
        .await
        .unwrap();

    let result1 = enroll_student(store.clone(), course_id.clone(), student1_id)
        .await
        .unwrap();
    assert!(result1);

    let student2_id = Uuid::new_v4().to_string();
    let result2 = enroll_student(store.clone(), course_id.clone(), student2_id)
        .await
        .unwrap();
    assert!(!result2);

    let course2_id = Uuid::new_v4().to_string();
    let student3_id = Uuid::new_v4().to_string();
    create_course(store.clone(), course2_id.clone(), 1)
        .await
        .unwrap();

    let result3 = enroll_student(store.clone(), course2_id, student3_id)
        .await
        .unwrap();
    assert!(result3);
}

#[tokio::test]
async fn append_async_with_after_sequence_position_should_detect_stale_reads_async() {
    let storage = Arc::new(InMemoryStorage::new());
    let store = Arc::new(MarmosaStore::new(storage.clone(), FakeClock::new(1000)));

    let course_id = Uuid::new_v4().to_string();
    create_course(store.clone(), course_id.clone(), 10)
        .await
        .unwrap();

    let query = Query {
        items: vec![QueryItem {
            tags: vec![Tag {
                key: "courseId".to_string(),
                value: course_id.clone(),
            }],
            event_types: vec![],
        }],
    };
    let events = store
        .read_async(query.clone(), None, None, None)
        .await
        .unwrap();
    let last_position = events.last().unwrap().position;

    // Concurrent append
    let concurrent_event = EventData {
        event_id: Uuid::new_v4().to_string(),
        event: DomainEvent {
            event_type: "StudentEnrolledToCourseEvent".to_string(),
            data: serde_json::to_string(&StudentEnrolled {
                course_id: course_id.clone(),
                student_id: Uuid::new_v4().to_string(),
            })
            .unwrap(),
            tags: vec![Tag {
                key: "courseId".to_string(),
                value: course_id.clone(),
            }],
        },
        metadata: None,
    };
    store
        .append_async(vec![concurrent_event], None)
        .await
        .unwrap();

    let new_event = EventData {
        event_id: Uuid::new_v4().to_string(),
        event: DomainEvent {
            event_type: "StudentEnrolledToCourseEvent".to_string(),
            data: serde_json::to_string(&StudentEnrolled {
                course_id: course_id.clone(),
                student_id: Uuid::new_v4().to_string(),
            })
            .unwrap(),
            tags: vec![Tag {
                key: "courseId".to_string(),
                value: course_id.clone(),
            }],
        },
        metadata: None,
    };

    let condition = AppendCondition {
        after_sequence_position: Some(last_position),
        fail_if_events_match: Query { items: vec![] }, // Empty query
    };

    let result = store.append_async(vec![new_event], Some(condition)).await;
    assert_eq!(result, Err(Error::AppendConditionFailed));
}

#[tokio::test]
async fn append_async_with_fail_if_events_match_should_detect_conflicting_events_async() {
    let storage = Arc::new(InMemoryStorage::new());
    let store = Arc::new(MarmosaStore::new(storage.clone(), FakeClock::new(1000)));

    let course_id = Uuid::new_v4().to_string();
    create_course(store.clone(), course_id.clone(), 10)
        .await
        .unwrap();

    let enroll_event = EventData {
        event_id: Uuid::new_v4().to_string(),
        event: DomainEvent {
            event_type: "StudentEnrolledToCourseEvent".to_string(),
            data: serde_json::to_string(&StudentEnrolled {
                course_id: course_id.clone(),
                student_id: Uuid::new_v4().to_string(),
            })
            .unwrap(),
            tags: vec![Tag {
                key: "courseId".to_string(),
                value: course_id.clone(),
            }],
        },
        metadata: None,
    };
    store.append_async(vec![enroll_event], None).await.unwrap();

    let conflict_query = Query {
        items: vec![QueryItem {
            tags: vec![Tag {
                key: "courseId".to_string(),
                value: course_id.clone(),
            }],
            event_types: vec!["StudentEnrolledToCourseEvent".to_string()],
        }],
    };

    let append_condition = AppendCondition {
        after_sequence_position: None,
        fail_if_events_match: conflict_query,
    };

    let new_event = EventData {
        event_id: Uuid::new_v4().to_string(),
        event: DomainEvent {
            event_type: "StudentEnrolledToCourseEvent".to_string(),
            data: serde_json::to_string(&StudentEnrolled {
                course_id: course_id.clone(),
                student_id: Uuid::new_v4().to_string(),
            })
            .unwrap(),
            tags: vec![Tag {
                key: "courseId".to_string(),
                value: course_id.clone(),
            }],
        },
        metadata: None,
    };

    let result = store
        .append_async(vec![new_event], Some(append_condition))
        .await;
    assert_eq!(result, Err(Error::AppendConditionFailed));
}
