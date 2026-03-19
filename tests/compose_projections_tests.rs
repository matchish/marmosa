use marmosa::decision_model::{build_decision_model_nary_from_events, DelegateDecisionProjection, DecisionProjection};
use marmosa::domain::{DomainEvent, EventRecord, Query, QueryItem, Tag};

fn make_event(payload: &str, position: u64, tags: Vec<(&str, &str)>) -> EventRecord {
    let tags = tags
        .into_iter()
        .map(|(k, v)| Tag {
            key: k.to_string(),
            value: v.to_string(),
        })
        .collect();

    EventRecord {
        position,
        event_id: format!("evt-{}", position),
        event: DomainEvent {
            event_type: payload.to_string(),
            data: "{}".to_string(),
            tags,
        },
        metadata: None,
        timestamp: 0,
    }
}

#[test]
fn compose2_non_overlapping_queries_each_state_updated_by_own_events() {
    let course_projection = DelegateDecisionProjection::new(
        false,
        Query {
            items: vec![QueryItem {
                event_types: vec!["CourseCreatedEvent".to_string()],
                tags: vec![],
            }],
        },
        |_, evt| evt.event.event_type == "CourseCreatedEvent",
    );

    let student_projection = DelegateDecisionProjection::new(
        false,
        Query {
            items: vec![QueryItem {
                event_types: vec!["StudentRegisteredEvent".to_string()],
                tags: vec![],
            }],
        },
        |_, evt| evt.event.event_type == "StudentRegisteredEvent",
    );

    let events = vec![
        make_event("CourseCreatedEvent", 1, vec![]),
        make_event("StudentRegisteredEvent", 2, vec![]),
    ];

    let projections: Vec<&dyn DecisionProjection<State = _>> = vec![&course_projection, &student_projection];
    let (states, condition) = build_decision_model_nary_from_events(&projections, &events);

    assert_eq!(states.len(), 2);
    assert_eq!(states[0], true);
    assert_eq!(states[1], true);
    assert_eq!(condition.after_sequence_position, Some(2));
}

#[test]
fn compose2_overlapping_queries_both_states_updated_by_shared_event() {
    let p1 = DelegateDecisionProjection::new(
        0,
        Query {
            items: vec![QueryItem {
                event_types: vec!["EnrolledEvent".to_string()],
                tags: vec![],
            }],
        },
        |s, evt| if evt.event.event_type == "EnrolledEvent" { s + 1 } else { s },
    );

    let p2 = DelegateDecisionProjection::new(
        0,
        Query {
            items: vec![QueryItem {
                event_types: vec!["EnrolledEvent".to_string()],
                tags: vec![],
            }],
        },
        |s, evt| if evt.event.event_type == "EnrolledEvent" { s + 1 } else { s },
    );

    let events = vec![
        make_event("EnrolledEvent", 1, vec![]),
    ];

    let projections: Vec<&dyn DecisionProjection<State = _>> = vec![&p1, &p2];
    let (states, condition) = build_decision_model_nary_from_events(&projections, &events);

    assert_eq!(states.len(), 2);
    assert_eq!(states[0], 1);
    assert_eq!(states[1], 1);
    assert_eq!(condition.after_sequence_position, Some(1));
}

#[test]
fn compose2_empty_event_store_returns_initial_states() {
    let p1 = DelegateDecisionProjection::new(
        false,
        Query {
            items: vec![QueryItem {
                event_types: vec!["CourseCreatedEvent".to_string()],
                tags: vec![],
            }],
        },
        |_, _| true,
    );

    let p2 = DelegateDecisionProjection::new(
        false,
        Query {
            items: vec![QueryItem {
                event_types: vec!["StudentRegisteredEvent".to_string()],
                tags: vec![],
            }],
        },
        |_, _| true,
    );

    let projections: Vec<&dyn DecisionProjection<State = _>> = vec![&p1, &p2];
    let (states, condition) = build_decision_model_nary_from_events(&projections, &[]);

    assert_eq!(states[0], false);
    assert_eq!(states[1], false);
    assert_eq!(condition.after_sequence_position, None);
}

#[test]
fn compose8_creates_combined_model_when_all_are_present() {
    let p1 = DelegateDecisionProjection::new(1, Query::all(), |s, _| s + 1);
    let p2 = DelegateDecisionProjection::new(2, Query::all(), |s, _| s + 1);
    let p3 = DelegateDecisionProjection::new(3, Query::all(), |s, _| s + 1);
    let p4 = DelegateDecisionProjection::new(4, Query::all(), |s, _| s + 1);
    let p5 = DelegateDecisionProjection::new(5, Query::all(), |s, _| s + 1);
    let p6 = DelegateDecisionProjection::new(6, Query::all(), |s, _| s + 1);
    let p7 = DelegateDecisionProjection::new(7, Query::all(), |s, _| s + 1);
    let p8 = DelegateDecisionProjection::new(8, Query::all(), |s, _| s + 1);

    let projections: Vec<&dyn DecisionProjection<State = i32>> = vec![&p1, &p2, &p3, &p4, &p5, &p6, &p7, &p8];

    let events = vec![
        make_event("Ev1", 1, vec![]),
        make_event("Ev2", 2, vec![]),
    ];

    let (states, condition) = build_decision_model_nary_from_events(&projections, &events);

    assert_eq!(states.len(), 8);
    // Initial + 2 events
    assert_eq!(states[0], 1 + 2);
    assert_eq!(states[1], 2 + 2);
    assert_eq!(states[2], 3 + 2);
    assert_eq!(states[3], 4 + 2);
    assert_eq!(states[4], 5 + 2);
    assert_eq!(states[5], 6 + 2);
    assert_eq!(states[6], 7 + 2);
    assert_eq!(states[7], 8 + 2);

    assert_eq!(condition.after_sequence_position, Some(2));
}