use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::borrow::Borrow;

use crate::domain::EventRecord;

use crate::domain::{AppendCondition, EventData, Query};
/// Extension trait for iterators to build projections from events.
use crate::event_store::EventStore;
use crate::ports::Error;

pub trait EventStoreExt {
    fn append_single_async(
        &self,
        event: EventData,
        condition: Option<AppendCondition>,
    ) -> impl core::future::Future<Output = Result<(), Error>> + Send;

    fn read_all_async(
        &self,
        query: Query,
    ) -> impl core::future::Future<Output = Result<Vec<EventRecord>, Error>> + Send;
}

impl<T: EventStore + Send + Sync> EventStoreExt for T {
    async fn append_single_async(
        &self,
        event: EventData,
        condition: Option<AppendCondition>,
    ) -> Result<(), Error> {
        self.append_async(alloc::vec![event], condition).await
    }

    async fn read_all_async(&self, query: Query) -> Result<Vec<EventRecord>, Error> {
        self.read_async(query, None, None, None).await
    }
}

pub struct DomainEventBuilder {
    event_type: String,
    data: String,
    tags: Vec<crate::domain::Tag>,
    metadata: Option<String>,
}

impl DomainEventBuilder {
    pub fn new(event_type: &str, data: &str) -> Self {
        Self {
            event_type: event_type.to_string(),
            data: data.to_string(),
            tags: alloc::vec::Vec::new(),
            metadata: None,
        }
    }

    pub fn with_tag(mut self, key: &str, value: &str) -> Self {
        self.tags.push(crate::domain::Tag {
            key: key.to_string(),
            value: value.to_string(),
        });
        self
    }

    pub fn with_tags(mut self, tags: Vec<crate::domain::Tag>) -> Self {
        self.tags.extend(tags);
        self
    }

    pub fn with_metadata(mut self, metadata: &str) -> Self {
        self.metadata = Some(metadata.to_string());
        self
    }

    pub fn build(self, event_id: &str) -> crate::domain::EventData {
        crate::domain::EventData {
            event_id: event_id.to_string(),
            event: crate::domain::DomainEvent {
                event_type: self.event_type,
                data: self.data,
                tags: self.tags,
            },
            metadata: self.metadata,
        }
    }
}

pub trait ToDomainEventExt {
    fn to_domain_event(&self) -> DomainEventBuilder;
}

impl<T: serde::Serialize> ToDomainEventExt for T {
    fn to_domain_event(&self) -> DomainEventBuilder {
        let type_name = core::any::type_name::<T>()
            .split("::")
            .last()
            .unwrap_or("UnknownType");
        let data = serde_json_core::to_vec::<_, 4096>(self)
            .map(|v| alloc::string::String::from_utf8(v.to_vec()).unwrap_or_else(|_| alloc::string::String::from("{}")))
            .unwrap_or_else(|_| alloc::string::String::from("{}"));
        DomainEventBuilder::new(type_name, &data)
    }
}

pub trait BuildProjectionsExt: Iterator {
    /// Builds projections by applying events grouped by a derived key.
    fn build_projections<TState, K, F>(self, key_selector: K, apply_event: F) -> Vec<TState>
    where
        Self: Sized,
        Self::Item: Borrow<EventRecord>,
        K: Fn(&EventRecord) -> Option<String>,
        F: Fn(Option<TState>, &EventRecord) -> Option<TState>;
}

impl<I> BuildProjectionsExt for I
where
    I: Iterator,
{
    fn build_projections<TState, K, F>(self, key_selector: K, apply_event: F) -> Vec<TState>
    where
        Self: Sized,
        Self::Item: Borrow<EventRecord>,
        K: Fn(&EventRecord) -> Option<String>,
        F: Fn(Option<TState>, &EventRecord) -> Option<TState>,
    {
        let mut states: BTreeMap<String, Option<TState>> = BTreeMap::new();

        for item in self {
            let event = item.borrow();
            if let Some(key) = key_selector(event) {
                let current_state = states.remove(&key).unwrap_or(None);
                let new_state = apply_event(current_state, event);
                states.insert(key, new_state);
            }
        }

        states.into_values().flatten().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{DomainEvent, Tag};
    use alloc::string::ToString;

    #[derive(Debug, Clone, PartialEq)]
    struct StudentProjection {
        student_id: String,
        name: String,
        email: String,
        course_count: i32,
    }

    impl StudentProjection {
        fn apply(current: Option<Self>, event: &EventRecord) -> Option<Self> {
            match event.event.event_type.as_str() {
                "StudentCreatedEvent" => {
                    let parts: Vec<&str> = event.event.data.split('|').collect();
                    let name = parts.get(0).copied().unwrap_or("").to_string();
                    let email = parts.get(1).copied().unwrap_or("").to_string();
                    let student_id = event
                        .event
                        .tags
                        .iter()
                        .find(|t| t.key == "studentId")
                        .unwrap()
                        .value
                        .clone();
                    Some(StudentProjection {
                        student_id,
                        name,
                        email,
                        course_count: 0,
                    })
                }
                "StudentEnrolledEvent" => current.map(|mut c| {
                    c.course_count += 1;
                    c
                }),
                "StudentNameChangedEvent" => {
                    let new_name = event.event.data.clone();
                    current.map(|mut c| {
                        c.name = new_name;
                        c
                    })
                }
                _ => current,
            }
        }
    }

    fn create_event(pos: u64, event_type: &str, data: &str, student_id: &str) -> EventRecord {
        EventRecord {
            position: pos,
            event_id: alloc::format!("evt-{}", pos),
            event: DomainEvent {
                event_type: event_type.to_string(),
                data: data.to_string(),
                tags: alloc::vec![Tag {
                    key: "studentId".to_string(),
                    value: student_id.to_string()
                }],
            },
            metadata: None,
            timestamp: 1000,
        }
    }

    #[test]
    fn build_projections_with_single_aggregate_builds_one_projection() {
        let student_id = "student-1";
        let events = alloc::vec![
            create_event(1, "StudentCreatedEvent", "Alice|alice@test.com", student_id),
            create_event(2, "StudentEnrolledEvent", "{}", student_id),
        ];

        let projections = events.iter().build_projections(
            |e| {
                e.event
                    .tags
                    .iter()
                    .find(|t| t.key == "studentId")
                    .map(|t| t.value.clone())
            },
            StudentProjection::apply,
        );

        assert_eq!(projections.len(), 1);
        assert_eq!(projections[0].student_id, student_id);
        assert_eq!(projections[0].name, "Alice");
        assert_eq!(projections[0].course_count, 1);
    }

    #[test]
    fn build_projections_with_multiple_entities_builds_multiple_projections() {
        let s1 = "student-1";
        let s2 = "student-2";
        let events = alloc::vec![
            create_event(1, "StudentCreatedEvent", "Alice|alice@test.com", s1),
            create_event(2, "StudentCreatedEvent", "Bob|bob@test.com", s2),
            create_event(3, "StudentEnrolledEvent", "{}", s1),
            create_event(4, "StudentEnrolledEvent", "{}", s2),
            create_event(5, "StudentEnrolledEvent", "{}", s2),
        ];

        let mut projections = events.iter().build_projections(
            |e| {
                e.event
                    .tags
                    .iter()
                    .find(|t| t.key == "studentId")
                    .map(|t| t.value.clone())
            },
            StudentProjection::apply,
        );

        projections.sort_by(|a, b| a.name.cmp(&b.name));

        assert_eq!(projections.len(), 2);
        assert_eq!(projections[0].name, "Alice");
        assert_eq!(projections[0].course_count, 1);

        assert_eq!(projections[1].name, "Bob");
        assert_eq!(projections[1].course_count, 2);
    }

    #[test]
    fn build_projections_applies_events_in_sequence() {
        let s1 = "student-1";
        let events = alloc::vec![
            create_event(1, "StudentCreatedEvent", "Alice|alice@test.com", s1),
            create_event(2, "StudentNameChangedEvent", "Alice Smith", s1),
            create_event(3, "StudentEnrolledEvent", "{}", s1),
            create_event(4, "StudentEnrolledEvent", "{}", s1),
        ];

        let projections = events.iter().build_projections(
            |e| {
                e.event
                    .tags
                    .iter()
                    .find(|t| t.key == "studentId")
                    .map(|t| t.value.clone())
            },
            StudentProjection::apply,
        );

        assert_eq!(projections.len(), 1);
        assert_eq!(projections[0].name, "Alice Smith");
        assert_eq!(projections[0].course_count, 2);
    }

    #[test]
    fn build_projections_with_empty_array_returns_empty() {
        let events: Vec<EventRecord> = alloc::vec![];

        let projections = events.iter().build_projections(
            |e| {
                e.event
                    .tags
                    .iter()
                    .find(|t| t.key == "studentId")
                    .map(|t| t.value.clone())
            },
            StudentProjection::apply,
        );

        assert!(projections.is_empty());
    }

    #[test]
    fn build_projections_handles_null_seed_state() {
        let s1 = "student-1";
        let events = alloc::vec![create_event(
            1,
            "StudentCreatedEvent",
            "Alice|alice@test.com",
            s1
        ),];

        let projections = events.iter().build_projections(
            |e| {
                e.event
                    .tags
                    .iter()
                    .find(|t| t.key == "studentId")
                    .map(|t| t.value.clone())
            },
            |current, event| {
                assert!(current.is_none());
                StudentProjection::apply(current, event)
            },
        );

        assert_eq!(projections.len(), 1);
    }

    #[test]
    fn build_projections_filters_out_null_projections() {
        let s1 = "student-1";
        let events = alloc::vec![create_event(
            1,
            "StudentCreatedEvent",
            "Alice|alice@test.com",
            s1
        ),];

        let projections = events.iter().build_projections::<StudentProjection, _, _>(
            |e| {
                e.event
                    .tags
                    .iter()
                    .find(|t| t.key == "studentId")
                    .map(|t| t.value.clone())
            },
            |_, _| None, // Always return None
        );

        assert!(projections.is_empty());
    }

    #[test]
    fn build_projections_with_interleaved_events_groups_correctly() {
        let s1 = "student-1";
        let s2 = "student-2";
        let events = alloc::vec![
            create_event(1, "StudentCreatedEvent", "Alice|alice@test.com", s1),
            create_event(2, "StudentCreatedEvent", "Bob|bob@test.com", s2),
            create_event(3, "StudentEnrolledEvent", "{}", s1),
            create_event(4, "StudentEnrolledEvent", "{}", s2),
            create_event(5, "StudentNameChangedEvent", "Alice Smith", s1),
        ];

        let mut projections = events.iter().build_projections(
            |e| {
                e.event
                    .tags
                    .iter()
                    .find(|t| t.key == "studentId")
                    .map(|t| t.value.clone())
            },
            StudentProjection::apply,
        );

        projections.sort_by(|a, b| a.name.cmp(&b.name));

        assert_eq!(projections.len(), 2);
        assert_eq!(projections[0].name, "Alice Smith");
        assert_eq!(projections[1].name, "Bob");
    }
}
