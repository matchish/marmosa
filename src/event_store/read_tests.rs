#[cfg(test)]
mod tests {
    use crate::domain::{DomainEvent, EventData, Query, Tag};
    use crate::event_store::{EventStore, OpossumStore};
    use crate::ports::tests::{FakeClock, InMemoryStorage};

    #[tokio::test]
    async fn test_read_async_with_query_all_returns_all_events() {
        let storage = InMemoryStorage::new();
        let clock = FakeClock::new(1696000000);
        let store = OpossumStore::new(storage, clock);

        for i in 1..=3 {
            let evt = EventData {
                event_id: alloc::format!("evt-{}", i),
                event: DomainEvent { event_type: "TestEvent".to_string(), data: alloc::format!("Data{}", i), tags: alloc::vec::Vec::new() },
                metadata: None,
            };
            store.append_async(alloc::vec::Vec::from([evt]), None).await.unwrap();
        }

        let events = store.read_async(Query::all(), None, None).await.unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].position, 0);
        assert_eq!(events[1].position, 1);
        assert_eq!(events[2].position, 2);
    }

    #[tokio::test]
    async fn test_read_async_with_single_event_type_returns_matching_events() {
        let storage = InMemoryStorage::new();
        let clock = FakeClock::new(1696000000);
        let store = OpossumStore::new(storage, clock);

        for event_type in ["OrderCreated", "OrderShipped", "OrderCreated"] {
            let evt = EventData {
                event_id: "id".to_string(),
                event: DomainEvent { event_type: event_type.to_string(), data: "test".to_string(), tags: alloc::vec::Vec::new() },
                metadata: None,
            };
            store.append_async(alloc::vec::Vec::from([evt]), None).await.unwrap();
        }

        let query = Query {
            items: alloc::vec::Vec::from([crate::domain::QueryItem {
                event_types: alloc::vec::Vec::from(["OrderCreated".to_string()]),
                tags: alloc::vec::Vec::new(),
            }])
        };

        let events = store.read_async(query, None, None).await.unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event.event_type, "OrderCreated");
        assert_eq!(events[1].event.event_type, "OrderCreated");
        assert_eq!(events[0].position, 0);
        assert_eq!(events[1].position, 2);
    }

    #[tokio::test]
    async fn test_read_async_with_single_tag_returns_matching_events() {
        let storage = InMemoryStorage::new();
        let clock = FakeClock::new(1696000000);
        let store = OpossumStore::new(storage, clock);

        let tag_prod = Tag { key: "Environment".to_string(), value: "Production".to_string() };
        let tag_dev = Tag { key: "Environment".to_string(), value: "Development".to_string() };

        for tag in [tag_prod.clone(), tag_dev, tag_prod.clone()] {
            let evt = EventData {
                event_id: "id".to_string(),
                event: DomainEvent { event_type: "TestEvent".to_string(), data: "test".to_string(), tags: alloc::vec::Vec::from([tag]) },
                metadata: None,
            };
            store.append_async(alloc::vec::Vec::from([evt]), None).await.unwrap();
        }

        let query = Query {
            items: alloc::vec::Vec::from([crate::domain::QueryItem {
                event_types: alloc::vec::Vec::new(),
                tags: alloc::vec::Vec::from([tag_prod]),
            }])
        };

        let events = store.read_async(query, None, None).await.unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].position, 0);
        assert_eq!(events[1].position, 2);
    }

    #[tokio::test]
    async fn test_read_async_with_from_position_in_middle_returns_only_events_after_position() {
        let storage = InMemoryStorage::new();
        let clock = FakeClock::new(1696000000);
        let store = OpossumStore::new(storage, clock);

        for i in 1..=5 {
            let evt = EventData {
                event_id: alloc::format!("evt-{}", i),
                event: DomainEvent { event_type: "TestEvent".to_string(), data: alloc::format!("Data{}", i), tags: alloc::vec::Vec::new() },
                metadata: None,
            };
            store.append_async(alloc::vec::Vec::from([evt]), None).await.unwrap();
        }

        let events = store.read_async(Query::all(), Some(1), None).await.unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].position, 2);
        assert_eq!(events[1].position, 3);
        assert_eq!(events[2].position, 4);
    }
}
