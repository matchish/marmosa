use crate::decision_model::DecisionProjection;
use crate::domain::{AppendCondition, EventRecord, Query};
use alloc::vec::Vec;

pub struct DecisionModel<TState> {
    pub state: TState,
    pub append_condition: AppendCondition,
}

pub fn build_decision_model_from_events<TState, P>(
    projection: &P,
    events: &[EventRecord],
) -> DecisionModel<TState>
where
    P: DecisionProjection<State = TState> + ?Sized,
{
    // The events must be folded in order of position
    let mut sorted_events: Vec<&EventRecord> = events.iter().collect();
    sorted_events.sort_by_key(|e| e.position);

    let state = sorted_events
        .iter()
        .fold(projection.initial_state(), |s, e| projection.apply(s, e));

    let max_position = sorted_events.last().map(|e| e.position);

    let append_condition = AppendCondition {
        fail_if_events_match: projection.query().clone(),
        after_sequence_position: max_position,
    };

    DecisionModel {
        state,
        append_condition,
    }
}

pub fn build_decision_model_nary_from_events<TState, P>(
    projections: &[&P],
    events: &[EventRecord],
) -> (Vec<TState>, AppendCondition)
where
    P: DecisionProjection<State = TState> + ?Sized,
{
    // Build union query
    let mut all_items = Vec::new();
    let mut is_all = false;

    for &projection in projections {
        let query = projection.query();
        if query.items.is_empty() {
            is_all = true;
            break;
        }
        for item in &query.items {
            if !all_items.contains(item) {
                all_items.push(item.clone());
            }
        }
    }

    let union_query = if is_all {
        Query::all()
    } else {
        Query { items: all_items }
    };

    let mut sorted_events: Vec<&EventRecord> = events.iter().collect();
    sorted_events.sort_by_key(|e| e.position);

    let max_position = sorted_events.last().map(|e| e.position);

    let states = projections
        .iter()
        .map(|&p| {
            sorted_events.iter().fold(p.initial_state(), |s, e| {
                if p.query().matches(e) {
                    p.apply(s, e)
                } else {
                    s
                }
            })
        })
        .collect();

    let append_condition = AppendCondition {
        fail_if_events_match: union_query,
        after_sequence_position: max_position,
    };

    (states, append_condition)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decision_model::DelegateDecisionProjection;
    use crate::domain::{DomainEvent, QueryItem, Tag};
    use alloc::string::{String, ToString};
    use alloc::vec;

    fn make_event(event_type: &str, position: u64, tags: Vec<(&str, &str)>) -> EventRecord {
        let tags = tags
            .into_iter()
            .map(|(k, v)| Tag {
                key: k.to_string(),
                value: v.to_string(),
            })
            .collect();

        EventRecord {
            position,
            event_id: alloc::format!("evt-{}", position),
            event: DomainEvent {
                event_type: event_type.to_string(),
                data: "{}".to_string(),
                tags,
            },
            metadata: None,
            timestamp: 0,
        }
    }

    #[test]
    fn decision_model_state_returns_constructor_value() {
        let condition = AppendCondition {
            fail_if_events_match: Query::all(),
            after_sequence_position: Some(5),
        };

        let model = DecisionModel {
            state: true,
            append_condition: condition,
        };

        assert_eq!(model.state, true);
    }

    #[test]
    fn decision_model_append_condition_returns_constructor_value() {
        let condition = AppendCondition {
            fail_if_events_match: Query::all(),
            after_sequence_position: Some(42),
        };

        let model = DecisionModel {
            state: 7,
            append_condition: condition,
        };

        assert_eq!(model.append_condition.after_sequence_position, Some(42));
    }

    #[test]
    fn build_empty_events_state_is_initial_state() {
        let query = Query {
            items: vec![QueryItem {
                event_types: vec!["CourseCreatedEvent".to_string()],
                tags: vec![],
            }],
        };

        let projection = DelegateDecisionProjection::new(false, query, |_, _| true);
        let model = build_decision_model_from_events(&projection, &[]);

        assert_eq!(model.state, false);
    }

    #[test]
    fn build_empty_events_append_condition_position_is_none() {
        let query = Query {
            items: vec![QueryItem {
                event_types: vec!["StudentEnrolledEvent".to_string()],
                tags: vec![],
            }],
        };

        let projection = DelegateDecisionProjection::new(0, query, |s, _| s + 1);
        let model = build_decision_model_from_events(&projection, &[]);

        assert_eq!(model.append_condition.after_sequence_position, None);
    }

    #[test]
    fn build_empty_events_fail_if_events_match_is_projection_query() {
        let query = Query {
            items: vec![QueryItem {
                event_types: vec!["CourseCreatedEvent".to_string()],
                tags: vec![],
            }],
        };

        let query_clone = query.clone();
        let projection = DelegateDecisionProjection::new(false, query, |s, _| s);
        let model = build_decision_model_from_events(&projection, &[]);

        assert_eq!(model.append_condition.fail_if_events_match.items.len(), 1);
        assert_eq!(
            model.append_condition.fail_if_events_match.items[0].event_types[0],
            "CourseCreatedEvent"
        );
    }

    #[test]
    fn build_single_matching_event_state_is_updated() {
        let query = Query {
            items: vec![QueryItem {
                event_types: vec!["CourseCreatedEvent".to_string()],
                tags: vec![],
            }],
        };

        let projection = DelegateDecisionProjection::new(false, query, |_, evt| {
            evt.event.event_type == "CourseCreatedEvent"
        });

        let evt = make_event("CourseCreatedEvent", 7, vec![]);
        let model = build_decision_model_from_events(&projection, &[evt]);

        assert_eq!(model.state, true);
    }

    #[test]
    fn build_single_event_after_sequence_position_equals_event_position() {
        let projection = DelegateDecisionProjection::new(false, Query::all(), |_, _| true);
        let model = build_decision_model_from_events(
            &projection,
            &[make_event("UnrelatedEvent", 13, vec![])],
        );

        assert_eq!(model.append_condition.after_sequence_position, Some(13));
    }

    #[test]
    fn build_multiple_events_state_reflects_all_applied() {
        let course_id = "course-123";

        let query = Query {
            items: vec![QueryItem {
                event_types: vec!["StudentEnrolledEvent".to_string()],
                tags: vec![],
            }],
        };

        let projection = DelegateDecisionProjection::new(0, query, |s, evt| {
            if evt.event.event_type == "StudentEnrolledEvent" {
                s + 1
            } else {
                s
            }
        });

        let events = vec![
            make_event("StudentEnrolledEvent", 1, vec![("courseId", course_id)]),
            make_event("StudentEnrolledEvent", 2, vec![("courseId", course_id)]),
            make_event("StudentEnrolledEvent", 3, vec![("courseId", course_id)]),
        ];

        let model = build_decision_model_from_events(&projection, &events);
        assert_eq!(model.state, 3);
    }

    #[test]
    fn build_multiple_events_after_sequence_position_is_max() {
        let projection = DelegateDecisionProjection::new(0, Query::all(), |s, _| s + 1);

        let events = vec![
            make_event("UnrelatedEvent", 2, vec![]),
            make_event("UnrelatedEvent", 7, vec![]),
            make_event("UnrelatedEvent", 4, vec![]),
        ];

        let model = build_decision_model_from_events(&projection, &events);
        assert_eq!(model.append_condition.after_sequence_position, Some(7));
    }

    #[test]
    fn build_events_are_applied_in_ascending_position_order() {
        let projection =
            DelegateDecisionProjection::new(Vec::new(), Query::all(), |mut list, evt| {
                list.push(evt.position);
                list
            });

        let events = vec![
            make_event("UnrelatedEvent", 5, vec![]),
            make_event("UnrelatedEvent", 1, vec![]),
            make_event("UnrelatedEvent", 3, vec![]),
        ];

        let model = build_decision_model_from_events(&projection, &events);
        assert_eq!(model.state, vec![1, 3, 5]);
    }

    #[test]
    fn nary_build_single_projection_state_matches_single_overload() {
        let course_id = "course-123";

        let projection1 = DelegateDecisionProjection::new(
            false,
            Query {
                items: vec![QueryItem {
                    event_types: vec!["CourseCreatedEvent".to_string()],
                    tags: vec![Tag {
                        key: "courseId".to_string(),
                        value: course_id.to_string(),
                    }],
                }],
            },
            |_, evt| evt.event.event_type == "CourseCreatedEvent",
        );

        let projections = vec![&projection1];
        let events = vec![make_event(
            "CourseCreatedEvent",
            1,
            vec![("courseId", course_id)],
        )];

        let (states, _condition) = build_decision_model_nary_from_events(&projections, &events);

        assert_eq!(states.len(), 1);
        assert_eq!(states[0], true);
    }

    #[test]
    fn nary_build_multiple_projections_each_gets_independent_state() {
        let course_id1 = "course-A";
        let course_id2 = "course-B";

        let projection1 = DelegateDecisionProjection::new(
            0,
            Query {
                items: vec![QueryItem {
                    event_types: vec!["StudentEnrolledEvent".to_string()],
                    tags: vec![Tag {
                        key: "courseId".to_string(),
                        value: course_id1.to_string(),
                    }],
                }],
            },
            |s, evt| {
                if evt.event.event_type == "StudentEnrolledEvent"
                    && evt.event.tags.iter().any(|t| t.value == course_id1)
                {
                    s + 1
                } else {
                    s
                }
            },
        );

        let projection2 = DelegateDecisionProjection::new(
            0,
            Query {
                items: vec![QueryItem {
                    event_types: vec!["StudentEnrolledEvent".to_string()],
                    tags: vec![Tag {
                        key: "courseId".to_string(),
                        value: course_id2.to_string(),
                    }],
                }],
            },
            |s, evt| {
                if evt.event.event_type == "StudentEnrolledEvent"
                    && evt.event.tags.iter().any(|t| t.value == course_id2)
                {
                    s + 1
                } else {
                    s
                }
            },
        );

        let projections: Vec<&dyn DecisionProjection<State = _>> = vec![&projection1, &projection2];

        let events = vec![
            make_event("StudentEnrolledEvent", 1, vec![("courseId", course_id1)]),
            make_event("StudentEnrolledEvent", 2, vec![("courseId", course_id1)]),
            make_event("StudentEnrolledEvent", 3, vec![("courseId", course_id2)]),
        ];

        let (states, _) = build_decision_model_nary_from_events(&projections, &events);

        assert_eq!(states.len(), 2);
        assert_eq!(states[0], 2); // two enrollments for course 1
        assert_eq!(states[1], 1); // one enrollment for course 2
    }

    #[test]
    fn nary_build_empty_event_set_all_states_are_initial_state() {
        let projection1 = DelegateDecisionProjection::new(
            false,
            Query {
                items: vec![QueryItem {
                    event_types: vec!["CourseCreatedEvent".to_string()],
                    tags: vec![],
                }],
            },
            |_, _| true,
        );

        let projection2 = DelegateDecisionProjection::new(
            false,
            Query {
                items: vec![QueryItem {
                    event_types: vec!["CourseCreatedEvent".to_string()],
                    tags: vec![],
                }],
            },
            |_, _| true,
        );

        let projections: Vec<&dyn DecisionProjection<State = _>> = vec![&projection1, &projection2];
        let (states, condition) = build_decision_model_nary_from_events(&projections, &[]);

        assert_eq!(states.len(), 2);
        assert_eq!(states[0], false);
        assert_eq!(states[1], false);
        assert_eq!(condition.after_sequence_position, None);
    }

    #[test]
    fn nary_build_append_condition_position_is_max_across_all_events() {
        let projection1 = DelegateDecisionProjection::new(false, Query::all(), |_, _| true);
        let projection2 = DelegateDecisionProjection::new(false, Query::all(), |_, _| true);

        let projections: Vec<&dyn DecisionProjection<State = _>> = vec![&projection1, &projection2];

        let events = vec![
            make_event("CourseCreatedEvent", 3, vec![]),
            make_event("StudentEnrolledEvent", 7, vec![]),
            make_event("UnrelatedEvent", 5, vec![]),
        ];

        let (_, condition) = build_decision_model_nary_from_events(&projections, &events);

        assert_eq!(condition.after_sequence_position, Some(7));
    }

    #[test]
    fn nary_build_states_order_matches_projections_order() {
        let projection1 = DelegateDecisionProjection::new(
            "none".to_string(),
            Query {
                items: vec![QueryItem {
                    event_types: vec!["CourseCreatedEvent".to_string()],
                    tags: vec![],
                }],
            },
            |_, _| "course-created".to_string(),
        );

        let projection2 = DelegateDecisionProjection::new(
            "none".to_string(),
            Query {
                items: vec![QueryItem {
                    event_types: vec!["StudentEnrolledEvent".to_string()],
                    tags: vec![],
                }],
            },
            |_, _| "student-enrolled".to_string(),
        );

        let projections: Vec<&dyn DecisionProjection<State = _>> = vec![&projection1, &projection2];

        let events = vec![
            make_event("CourseCreatedEvent", 1, vec![]),
            make_event("StudentEnrolledEvent", 2, vec![]),
        ];

        let (states, _) = build_decision_model_nary_from_events(&projections, &events);

        assert_eq!(states[0], "course-created");
        assert_eq!(states[1], "student-enrolled");
    }
}
