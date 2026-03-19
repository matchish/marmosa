use marmosa::domain::{DomainEvent, EventRecord, Query, QueryItem};
use marmosa::event_store::EventStore;
use marmosa::projections::related_events::ProjectionWithRelatedEvents;
use marmosa::projections::{ProjectionDefinition, ProjectionRunner, ProjectionStore};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

mod common;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct UserCreatedEvent {
    user_id: String,
    name: String,
    email: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct UserNameChangedEvent {
    user_id: String,
    new_name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct PostCreatedEvent {
    post_id: String,
    author_id: String,
    title: String,
    content: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct PostTitleChangedEvent {
    post_id: String,
    new_title: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct PostWithAuthorState {
    post_id: String,
    title: String,
    content: String,
    author_id: String,
    author_name: String,
    author_email: String,
}

struct PostWithAuthorProjection;

impl ProjectionDefinition for PostWithAuthorProjection {
    type State = PostWithAuthorState;

    fn projection_name(&self) -> &str {
        "PostsWithAuthor"
    }

    fn event_types(&self) -> Query {
        Query {
            items: vec![QueryItem {
                event_types: vec![
                    "PostCreatedEvent".to_string(),
                    "PostTitleChangedEvent".to_string(),
                ],
                tags: vec![],
            }],
        }
    }

    fn key_selector(&self, event: &EventRecord) -> Option<String> {
        match event.event.event_type.as_str() {
            "PostCreatedEvent" => {
                let pce: PostCreatedEvent = serde_json::from_str(&event.event.data).unwrap();
                Some(pce.post_id)
            }
            "PostTitleChangedEvent" => {
                let ptce: PostTitleChangedEvent = serde_json::from_str(&event.event.data).unwrap();
                Some(ptce.post_id)
            }
            _ => None,
        }
    }

    fn apply(&self, _: Option<Self::State>, _: &EventRecord) -> Option<Self::State> {
        panic!("Should use apply_with_related");
    }

    fn as_related_events(&self) -> Option<&dyn ProjectionWithRelatedEvents<State = Self::State>> {
        Some(self)
    }
}

impl ProjectionWithRelatedEvents for PostWithAuthorProjection {
    fn get_related_events_query(&self, event: &EventRecord) -> Option<Query> {
        match event.event.event_type.as_str() {
            "PostCreatedEvent" => {
                let pce: PostCreatedEvent = serde_json::from_str(&event.event.data).unwrap();
                Some(Query {
                    items: vec![QueryItem {
                        event_types: vec![
                            "UserCreatedEvent".to_string(),
                            "UserNameChangedEvent".to_string(),
                        ],
                        tags: vec![marmosa::domain::Tag {
                            key: "userId".to_string(),
                            value: pce.author_id,
                        }],
                    }],
                })
            }
            _ => None,
        }
    }

    fn apply_with_related(
        &self,
        current_state: Option<Self::State>,
        event: &EventRecord,
        related_events: &[EventRecord],
    ) -> Option<Self::State> {
        match event.event.event_type.as_str() {
            "PostCreatedEvent" => {
                let pce: PostCreatedEvent = serde_json::from_str(&event.event.data).unwrap();

                let mut user_name = String::new();
                let mut user_email = String::new();

                let mut sorted_events = related_events.to_vec();
                sorted_events.sort_by_key(|e| e.position);

                for related_event in sorted_events {
                    match related_event.event.event_type.as_str() {
                        "UserCreatedEvent" => {
                            let uce: UserCreatedEvent =
                                serde_json::from_str(&related_event.event.data).unwrap();
                            if uce.user_id == pce.author_id {
                                user_name = uce.name.clone();
                                user_email = uce.email.clone();
                            }
                        }
                        "UserNameChangedEvent" => {
                            let uce: UserNameChangedEvent =
                                serde_json::from_str(&related_event.event.data).unwrap();
                            if uce.user_id == pce.author_id {
                                user_name = uce.new_name.clone();
                            }
                        }
                        _ => {}
                    }
                }

                Some(PostWithAuthorState {
                    post_id: pce.post_id,
                    title: pce.title,
                    content: pce.content,
                    author_id: pce.author_id,
                    author_name: user_name,
                    author_email: user_email,
                })
            }
            "PostTitleChangedEvent" => {
                let ptce: PostTitleChangedEvent = serde_json::from_str(&event.event.data).unwrap();
                current_state.map(|mut state| {
                    state.title = ptce.new_title;
                    state
                })
            }
            _ => current_state,
        }
    }
}

#[tokio::test]
async fn projection_with_related_events_builds_correct_state() {
    let storage = Arc::new(common::InMemoryStorage::new());
    let clock = common::FakeClock::new(100);
    let store = marmosa::event_store::MarmosaStore::new(storage.clone(), clock);

    let user_id = Uuid::new_v4().to_string();
    let post_id = Uuid::new_v4().to_string();

    let user_created = marmosa::domain::EventData {
        event_id: uuid::Uuid::new_v4().to_string(),
        metadata: None,
        event: DomainEvent {
            event_type: "UserCreatedEvent".to_string(),
            data: serde_json::to_string(&UserCreatedEvent {
                user_id: user_id.clone(),
                name: "John Doe".to_string(),
                email: "john@example.com".to_string(),
            })
            .unwrap(),
            tags: vec![marmosa::domain::Tag {
                key: "userId".to_string(),
                value: user_id.clone(),
            }],
        },
    };

    store.append_async(vec![user_created], None).await.unwrap();

    let post_created = marmosa::domain::EventData {
        event_id: uuid::Uuid::new_v4().to_string(),
        metadata: None,
        event: DomainEvent {
            event_type: "PostCreatedEvent".to_string(),
            data: serde_json::to_string(&PostCreatedEvent {
                post_id: post_id.clone(),
                author_id: user_id.clone(),
                title: "My First Post".to_string(),
                content: "Hello World".to_string(),
            })
            .unwrap(),
            tags: vec![
                marmosa::domain::Tag {
                    key: "postId".to_string(),
                    value: post_id.clone(),
                },
                marmosa::domain::Tag {
                    key: "userId".to_string(),
                    value: user_id.clone(),
                },
            ],
        },
    };

    store.append_async(vec![post_created], None).await.unwrap();

    let projection = PostWithAuthorProjection;
    let proj_store = marmosa::projections::StorageBackendProjectionStore::new(
        Arc::new(storage.clone()),
        projection.projection_name().to_string(),
    );

    let runner = ProjectionRunner::new(storage.clone(), PostWithAuthorProjection, proj_store);
    runner.rebuild(&store).await.unwrap();

    let read_store = marmosa::projections::StorageBackendProjectionStore::new(
        storage.clone(),
        "PostsWithAuthor".to_string(),
    );
    let state: PostWithAuthorState = read_store.get(&post_id).await.unwrap().unwrap();
    assert_eq!(state.post_id, post_id);
    assert_eq!(state.title, "My First Post");
    assert_eq!(state.content, "Hello World");
    assert_eq!(state.author_id, user_id);
    assert_eq!(state.author_name, "John Doe");
    assert_eq!(state.author_email, "john@example.com");
}
