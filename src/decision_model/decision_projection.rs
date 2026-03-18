use crate::domain::{EventRecord, Query};
use crate::ports::Clock;

pub trait DecisionProjection {
    type State;

    fn initial_state(&self) -> Self::State;
    fn query(&self) -> &Query;
    fn apply(&self, state: Self::State, event: &EventRecord) -> Self::State;
}

pub struct DelegateDecisionProjection<TState, F> {
    pub initial_state: TState,
    pub query: Query,
    apply_fn: F,
}

impl<TState: Clone, F> DelegateDecisionProjection<TState, F>
where
    F: Fn(TState, &EventRecord) -> TState,
{
    pub fn new(initial_state: TState, query: Query, apply_fn: F) -> Self {
        Self {
            initial_state,
            query,
            apply_fn,
        }
    }
}

impl<TState: Clone, F> DecisionProjection for DelegateDecisionProjection<TState, F>
where
    F: Fn(TState, &EventRecord) -> TState,
{
    type State = TState;

    fn initial_state(&self) -> Self::State {
        self.initial_state.clone()
    }

    fn query(&self) -> &Query {
        &self.query
    }

    fn apply(&self, state: Self::State, event: &EventRecord) -> Self::State {
        (self.apply_fn)(state, event)
    }
}

pub struct TimeAwareDecisionProjection<TState, C, F> {
    pub initial_state: TState,
    pub query: Query,
    pub clock: C,
    apply_fn: F,
}

impl<TState: Clone, C: Clock, F> TimeAwareDecisionProjection<TState, C, F>
where
    F: Fn(TState, &EventRecord, &C) -> TState,
{
    pub fn new(initial_state: TState, query: Query, clock: C, apply_fn: F) -> Self {
        Self {
            initial_state,
            query,
            clock,
            apply_fn,
        }
    }
}

impl<TState: Clone, C: Clock, F> DecisionProjection for TimeAwareDecisionProjection<TState, C, F>
where
    F: Fn(TState, &EventRecord, &C) -> TState,
{
    type State = TState;

    fn initial_state(&self) -> Self::State {
        self.initial_state.clone()
    }

    fn query(&self) -> &Query {
        &self.query
    }

    fn apply(&self, state: Self::State, event: &EventRecord) -> Self::State {
        (self.apply_fn)(state, event, &self.clock)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{DomainEvent, QueryItem, Tag};
    use crate::ports::tests::FakeClock;
    use alloc::string::{String, ToString};
    use alloc::vec;

    fn make_sequenced_event(event_type: &str, position: u64, tags: vec::Vec<Tag>) -> EventRecord {
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

    fn make_timestamped_event(
        event_type: &str,
        timestamp: u64,
        position: u64,
    ) -> EventRecord {
        EventRecord {
            position,
            event_id: alloc::format!("evt-{}", position),
            event: DomainEvent {
                event_type: event_type.to_string(),
                data: "{}".to_string(),
                tags: vec![],
            },
            metadata: None,
            timestamp,
        }
    }

    #[test]
    fn initial_state_returns_value_passed_to_constructor() {
        let projection = DelegateDecisionProjection::new(42, Query::all(), |s, _| s);
        assert_eq!(projection.initial_state(), 42);
    }

    #[test]
    fn initial_state_false_returns_correctly() {
        let projection = DelegateDecisionProjection::new(false, Query::all(), |s, _| s);
        assert_eq!(projection.initial_state(), false);
    }

    #[test]
    fn initial_state_none_is_allowed() {
        let projection: DelegateDecisionProjection<Option<String>, _> =
            DelegateDecisionProjection::new(None, Query::all(), |s, _| s);
        assert_eq!(projection.initial_state(), None);
    }

    #[test]
    fn query_returns_query_passed_to_constructor() {
        let query = Query {
            items: vec![QueryItem {
                event_types: vec!["CourseCreated".to_string()],
                tags: vec![],
            }],
        };
        let projection = DelegateDecisionProjection::new(false, query.clone(), |s, _| s);
        assert_eq!(projection.query().items[0].event_types[0], "CourseCreated");
    }

    #[test]
    fn apply_returns_value_from_delegate() {
        let projection = DelegateDecisionProjection::new(0, Query::all(), |s, _| s + 10);
        let event = make_sequenced_event("TestEvent", 1, vec![]);
        let result = projection.apply(0, &event);
        assert_eq!(result, 10);
    }

    #[test]
    fn apply_when_event_type_does_not_match_can_return_unchanged_state() {
        let projection = DelegateDecisionProjection::new(false, Query::all(), |state, evt| {
            if evt.event.event_type == "TestEvent" {
                true
            } else {
                state
            }
        });
        let evt = make_sequenced_event("OtherEvent", 1, vec![]);
        let result = projection.apply(false, &evt);
        assert_eq!(result, false);
    }

    #[test]
    fn apply_multiple_calls_folds_state_correctly() {
        let projection = DelegateDecisionProjection::new(0, Query::all(), |state, evt| {
            if evt.event.event_type == "TestEvent" {
                state + 1
            } else {
                state
            }
        });

        let events = vec![
            make_sequenced_event("TestEvent", 1, vec![]),
            make_sequenced_event("TestEvent", 2, vec![]),
            make_sequenced_event("OtherEvent", 3, vec![]),
            make_sequenced_event("TestEvent", 4, vec![]),
        ];

        let mut state = projection.initial_state();
        for evt in &events {
            state = projection.apply(state, evt);
        }

        assert_eq!(state, 3);
    }

    #[test]
    fn factory_function_produces_correct_projection() {
        fn course_exists(course_id: String) -> impl DecisionProjection<State = bool> {
            DelegateDecisionProjection::new(
                false,
                Query {
                    items: vec![QueryItem {
                        event_types: vec!["CourseCreated".to_string()],
                        tags: vec![Tag {
                            key: "courseId".to_string(),
                            value: course_id,
                        }],
                    }],
                },
                |state, evt| {
                    if evt.event.event_type == "CourseCreated" {
                        true
                    } else {
                        state
                    }
                },
            )
        }

        let id = "1234-5678".to_string();
        let projection = course_exists(id.clone());

        assert_eq!(projection.initial_state(), false);
        assert_eq!(projection.query().items.len(), 1);
        assert_eq!(projection.query().items[0].event_types[0], "CourseCreated");
        assert_eq!(projection.query().items[0].tags[0].key, "courseId");
        assert_eq!(projection.query().items[0].tags[0].value, id);
    }

    #[test]
    fn two_projections_same_events_produce_independent_states() {
        let count_projection = DelegateDecisionProjection::new(0, Query::all(), |state, evt| {
            if evt.event.event_type == "TestEvent" {
                state + 1
            } else {
                state
            }
        });

        let last_value_projection =
            DelegateDecisionProjection::new(-1, Query::all(), |state: i32, evt| {
                if evt.event.event_type == "OtherEvent" {
                    // Extract value from data if it was real, but here we just mock it
                    // The test used "make_sequenced_event(new OtherEvent(10))" so let's mock the update
                    if evt.position == 2 {
                        return 10;
                    }
                    if evt.position == 4 {
                        return 42;
                    }
                }
                state
            });

        let events = vec![
            make_sequenced_event("TestEvent", 1, vec![]),
            make_sequenced_event("OtherEvent", 2, vec![]),
            make_sequenced_event("TestEvent", 3, vec![]),
            make_sequenced_event("OtherEvent", 4, vec![]),
        ];

        let mut count_state = count_projection.initial_state();
        let mut last_value_state = last_value_projection.initial_state();

        for evt in &events {
            // Simplified matching logic for the sake of the test fold
            count_state = count_projection.apply(count_state, evt);
            last_value_state = last_value_projection.apply(last_value_state, evt);
        }

        assert_eq!(count_state, 2);
        assert_eq!(last_value_state, 42);
    }

    #[test]
    fn time_aware_projection_within_grace_period_returns_new_state() {
        let event_time = 1_000_000;
        let grace_period = 30 * 60 * 1000; // 30 minutes in ms

        // now is 5 minutes after the event — within grace
        let current_time = event_time + 5 * 60 * 1000;
        let clock = FakeClock::new(current_time);

        let projection = TimeAwareDecisionProjection::new(
            false,
            Query::all(),
            clock,
            |_, evt, clock| {
                let now = clock.now_millis();
                let age = now - evt.timestamp;
                age <= grace_period
            },
        );

        let evt = make_timestamped_event("PriceChanged", event_time, 1);
        let result = projection.apply(projection.initial_state(), &evt);

        assert_eq!(result, true);
    }

    #[test]
    fn time_aware_projection_after_grace_period_expired_returns_old_state() {
        let event_time = 1_000_000;
        let grace_period = 30 * 60 * 1000; // 30 minutes in ms

        // now is 2 hours after the event — grace period expired
        let current_time = event_time + 2 * 60 * 60 * 1000;
        let clock = FakeClock::new(current_time);

        let projection = TimeAwareDecisionProjection::new(
            false,
            Query::all(),
            clock,
            |_, evt, clock| {
                let now = clock.now_millis();
                let age = now - evt.timestamp;
                age <= grace_period
            },
        );

        let evt = make_timestamped_event("PriceChanged", event_time, 1);
        let result = projection.apply(projection.initial_state(), &evt);

        assert_eq!(result, false);
    }
}
