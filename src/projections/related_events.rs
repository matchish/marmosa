use crate::domain::{EventRecord, Query};
use crate::projections::ProjectionDefinition;

/// Defines a projection that fetches related events from the event store
/// to build its state. The framework will automatically load related events
/// before calling `apply_with_related`.
pub trait ProjectionWithRelatedEvents: ProjectionDefinition + Send + Sync {
    /// Determines what related events to load for a given event.
    /// Called by the framework before apply to fetch additional context.
    fn get_related_events_query(&self, event: &EventRecord) -> Option<Query>;

    /// Applies an event to the current projection state with access to related events.
    /// The framework guarantees that related events (from `get_related_events_query`) are loaded.
    /// In many cases, IProjectionWithRelatedEvents overrides the base `apply` to panic.
    fn apply_with_related(
        &self,
        state: Option<Self::State>,
        event: &EventRecord,
        related_events: &[EventRecord],
    ) -> Option<Self::State>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{DomainEvent, QueryItem, Tag};
    use alloc::string::{String, ToString};
    use alloc::vec;
    use alloc::vec::Vec;

    #[derive(Debug, Clone, PartialEq)]
    struct TestState {
        main_data: String,
        related_data: String,
        update_count: i32,
    }

    struct TestProjectionWithRelatedEvents;

    impl ProjectionDefinition for TestProjectionWithRelatedEvents {
        type State = TestState;

        fn projection_name(&self) -> &str {
            "TestProjectionWithRelatedEvents"
        }

        fn event_types(&self) -> Query {
            Query {
                items: vec![QueryItem {
                    event_types: vec!["TestEvent".to_string(), "TestEventWithRelation".to_string()],
                    tags: vec![],
                }],
            }
        }

        fn key_selector(&self, event: &EventRecord) -> Option<String> {
            Some(event.position.to_string())
        }

        fn apply(&self, _state: Option<Self::State>, _event: &EventRecord) -> Option<Self::State> {
            unimplemented!(
                "Projection TestProjectionWithRelatedEvents must implement apply_with_related. The base apply method is hidden/panics."
            )
        }
    }

    impl ProjectionWithRelatedEvents for TestProjectionWithRelatedEvents {
        fn get_related_events_query(&self, event: &EventRecord) -> Option<Query> {
            if event.event.event_type == "TestEventWithRelation" {
                if let Some(related_id_tag) = event.event.tags.iter().find(|t| t.key == "RelatedId")
                {
                    return Some(Query {
                        items: vec![QueryItem {
                            tags: vec![Tag {
                                key: "relatedId".to_string(),
                                value: related_id_tag.value.clone(),
                            }],
                            event_types: vec!["RelatedEvent".to_string()],
                        }],
                    });
                }
            }
            None
        }

        fn apply_with_related(
            &self,
            state: Option<Self::State>,
            event: &EventRecord,
            related_events: &[EventRecord],
        ) -> Option<Self::State> {
            match event.event.event_type.as_str() {
                "TestEvent" => {
                    let mut current = state.unwrap_or(TestState {
                        main_data: event.event.data.clone(),
                        related_data: "".to_string(),
                        update_count: 0,
                    });
                    current.main_data = event.event.data.clone();
                    current.update_count += 1;
                    Some(current)
                }
                "TestEventWithRelation" => {
                    let related_event = related_events
                        .iter()
                        .filter(|e| e.event.event_type == "RelatedEvent")
                        .max_by_key(|e| e.position)
                        .expect("Related event not found");

                    Some(TestState {
                        main_data: event.event.data.clone(),
                        related_data: related_event.event.data.clone(),
                        update_count: 1,
                    })
                }
                _ => state,
            }
        }
    }

    fn create_sequenced_event(event_type: &str, data: &str, tags: Vec<Tag>) -> EventRecord {
        EventRecord {
            position: 1,
            event_id: "test-id".to_string(),
            event: DomainEvent {
                event_type: event_type.to_string(),
                data: data.to_string(),
                tags,
            },
            metadata: None,
            timestamp: 0,
        }
    }

    #[test]
    #[should_panic(
        expected = "Projection TestProjectionWithRelatedEvents must implement apply_with_related"
    )]
    fn projection_with_related_events_base_apply_method_panics() {
        let projection = TestProjectionWithRelatedEvents;
        let evt = create_sequenced_event("TestEvent", "test", vec![]);
        projection.apply(None, &evt);
    }

    #[test]
    fn projection_with_related_events_get_related_events_query_returns_correct_query() {
        let projection = TestProjectionWithRelatedEvents;
        let evt = create_sequenced_event(
            "TestEventWithRelation",
            "test",
            vec![Tag {
                key: "RelatedId".to_string(),
                value: "guid-123".to_string(),
            }],
        );

        let query = projection.get_related_events_query(&evt).unwrap();
        assert_eq!(query.items.len(), 1);
        assert_eq!(query.items[0].event_types[0], "RelatedEvent");
        assert_eq!(query.items[0].tags[0].value, "guid-123");
    }

    #[test]
    fn projection_with_related_events_get_related_events_query_with_no_relationship_returns_none() {
        let projection = TestProjectionWithRelatedEvents;
        let evt = create_sequenced_event("TestEvent", "test", vec![]);

        let query = projection.get_related_events_query(&evt);
        assert!(query.is_none());
    }

    #[test]
    fn projection_with_related_events_apply_with_related_events_uses_related_data() {
        let projection = TestProjectionWithRelatedEvents;
        let evt = create_sequenced_event(
            "TestEventWithRelation",
            "main",
            vec![Tag {
                key: "RelatedId".to_string(),
                value: "guid-123".to_string(),
            }],
        );

        let related_event = EventRecord {
            position: 1,
            event_id: "rel-id".to_string(),
            event: DomainEvent {
                event_type: "RelatedEvent".to_string(),
                data: "Related Data".to_string(),
                tags: vec![],
            },
            metadata: None,
            timestamp: 0,
        };

        let state = projection
            .apply_with_related(None, &evt, &[related_event])
            .unwrap();
        assert_eq!(state.main_data, "main");
        assert_eq!(state.related_data, "Related Data");
    }

    #[test]
    #[should_panic(expected = "Related event not found")]
    fn projection_with_related_events_apply_without_required_related_events_panics() {
        let projection = TestProjectionWithRelatedEvents;
        let evt = create_sequenced_event(
            "TestEventWithRelation",
            "main",
            vec![Tag {
                key: "RelatedId".to_string(),
                value: "guid-123".to_string(),
            }],
        );

        projection.apply_with_related(None, &evt, &[]);
    }

    #[test]
    fn projection_with_related_events_apply_updates_existing_state() {
        let projection = TestProjectionWithRelatedEvents;
        let existing_state = TestState {
            main_data: "old".to_string(),
            related_data: "old related".to_string(),
            update_count: 1,
        };

        let evt = create_sequenced_event("TestEvent", "updated", vec![]);

        let new_state = projection
            .apply_with_related(Some(existing_state), &evt, &[])
            .unwrap();

        assert_eq!(new_state.main_data, "updated");
        assert_eq!(new_state.related_data, "old related"); // Preserved
        assert_eq!(new_state.update_count, 2); // Incremented
    }

    #[test]
    fn projection_with_related_events_apply_with_multiple_related_events_uses_latest() {
        let projection = TestProjectionWithRelatedEvents;
        let evt = create_sequenced_event(
            "TestEventWithRelation",
            "main",
            vec![Tag {
                key: "RelatedId".to_string(),
                value: "guid-123".to_string(),
            }],
        );

        let evt1 = EventRecord {
            position: 1,
            event_id: "evt1".to_string(),
            event: DomainEvent {
                event_type: "RelatedEvent".to_string(),
                data: "First".to_string(),
                tags: vec![],
            },
            metadata: None,
            timestamp: 0,
        };

        let evt2 = EventRecord {
            position: 2,
            event_id: "evt2".to_string(),
            event: DomainEvent {
                event_type: "RelatedEvent".to_string(),
                data: "Second".to_string(),
                tags: vec![],
            },
            metadata: None,
            timestamp: 0,
        };

        let evt3 = EventRecord {
            position: 3,
            event_id: "evt3".to_string(),
            event: DomainEvent {
                event_type: "RelatedEvent".to_string(),
                data: "Latest".to_string(),
                tags: vec![],
            },
            metadata: None,
            timestamp: 0,
        };

        let state = projection
            .apply_with_related(None, &evt, &[evt1, evt2, evt3])
            .unwrap();

        assert_eq!(state.main_data, "main");
        assert_eq!(state.related_data, "Latest"); // Should use last one
    }
}
