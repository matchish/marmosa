mod common;

use common::{FakeClock, InMemoryStorage};
use marmosa::domain::{DomainEvent, EventData, EventRecord, Query};
use marmosa::event_store::{EventStore, OpossumStore};
use marmosa::ports::StorageBackend;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OrphanedEvent {
    data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PostRecoveryEvent {
    data: String,
}

#[tokio::test]
async fn append_after_crash_preserves_orphaned_events_and_allocates_next_positions() {
    let storage = Arc::new(InMemoryStorage::new());
    
    // -- Arrange: append 2 events normally --
    let store = OpossumStore::new(Arc::clone(&storage), FakeClock::new(1000));

    let pre_event1 = EventData {
        event_id: uuid::Uuid::new_v4().to_string(),
        event: DomainEvent {
            event_type: "OrphanedEvent".to_string(),
            data: serde_json::to_string(&OrphanedEvent { data: "pre-crash-1".to_string() }).unwrap(),
            tags: vec![],
        },
        metadata: None,
    };
    
    let pre_event2 = EventData {
        event_id: uuid::Uuid::new_v4().to_string(),
        event: DomainEvent {
            event_type: "OrphanedEvent".to_string(),
            data: serde_json::to_string(&OrphanedEvent { data: "pre-crash-2".to_string() }).unwrap(),
            tags: vec![],
        },
        metadata: None,
    };

    store.append_async(vec![pre_event1, pre_event2], None).await.unwrap();

    // Verify: ledger at position 2
    let events_before_crash = store.read_async(Query::all(), None, None, None).await.unwrap();
    println!("Files: {:?}", storage.read_dir("Events").await);
    println!("Events: {:?}", events_before_crash);
    assert_eq!(events_before_crash.len(), 2);

    // -- Simulate crash: write event file at position 3 without updating ledger --
    let events_path = "Events";
    storage.create_dir_all(events_path).await.unwrap(); // Make sure dir exists

    let orphaned_sequenced = EventRecord {
        position: 3,
        event_id: uuid::Uuid::new_v4().to_string(),
        event: DomainEvent {
            event_type: "OrphanedEvent".to_string(),
            data: serde_json::to_string(&OrphanedEvent { data: "orphaned-crash-event".to_string() }).unwrap(),
            tags: vec![],
        },
        metadata: None,
        timestamp: 1000,
    };
    
    // Write directly using storage backend to bypass store.
    let event_file_path = format!("{}/0000000003.json", events_path);
    let serialized = serde_json_core::to_vec::<_, 2048>(&orphaned_sequenced).unwrap();
    storage.write_file(&event_file_path, &serialized).await.unwrap();

    // In Rust we don't have direct access to internal ledger state easily here without parsing internal files, 
    // but we know it's at 2 because we bypassed the store for event 3.

    // -- Act: create a NEW store instance (simulates restart) and append --
    let recovered_store = OpossumStore::new(Arc::clone(&storage), FakeClock::new(1000));
    
    let post_event = EventData {
        event_id: uuid::Uuid::new_v4().to_string(),
        event: DomainEvent {
            event_type: "PostRecoveryEvent".to_string(),
            data: serde_json::to_string(&PostRecoveryEvent { data: "post-recovery".to_string() }).unwrap(),
            tags: vec![],
        },
        metadata: None,
    };
    recovered_store.append_async(vec![post_event], None).await.unwrap();

    // -- Assert: orphaned event at position 3 is preserved, new event at position 4 --
    let all_events = recovered_store.read_async(Query::all(), None, None, None).await.unwrap();
    
    assert_eq!(all_events.len(), 4);
    assert_eq!(all_events[0].position, 0);
    assert_eq!(all_events[1].position, 1);
    assert_eq!(all_events[2].position, 3);
    assert_eq!(all_events[3].position, 4);

    let orphan_read: OrphanedEvent = serde_json::from_str(&all_events[2].event.data.replace("\\\"", "\"").trim_matches('"')).unwrap();
    assert_eq!(orphan_read.data, "orphaned-crash-event");
    
    let post_read: PostRecoveryEvent = serde_json::from_str(&all_events[3].event.data.replace("\\\"", "\"").trim_matches('"')).unwrap();
    assert_eq!(post_read.data, "post-recovery");
}

#[tokio::test]
async fn read_after_crash_recovery_returns_all_events_in_correct_order() {
    let storage = Arc::new(InMemoryStorage::new());

    // -- Arrange: append 3 events normally --
    let store = OpossumStore::new(Arc::clone(&storage), FakeClock::new(1000));

    let events: Vec<EventData> = (1..=3)
        .map(|i| EventData {
            event_id: uuid::Uuid::new_v4().to_string(),
            event: DomainEvent {
                event_type: "OrphanedEvent".to_string(),
                data: serde_json::to_string(&OrphanedEvent { data: format!("normal-{}", i) }).unwrap(),
                tags: vec![],
            },
            metadata: None,
        })
        .collect();

    store.append_async(events, None).await.unwrap();

    // -- Simulate crash: write 2 orphaned event files at positions 4 and 5 --
    let events_path = "Events";
    storage.create_dir_all(events_path).await.unwrap(); // Make sure dir exists

    for i in 3..=4 {
        let orphan = EventRecord {
            position: i,
            event_id: uuid::Uuid::new_v4().to_string(),
            event: DomainEvent {
                event_type: "OrphanedEvent".to_string(),
                data: serde_json::to_string(&OrphanedEvent { data: format!("orphaned-{}", i) }).unwrap(),
                tags: vec![],
            },
            metadata: None,
            timestamp: 1000,
        };
        
        // Write directly bypass store.
        let event_file_path = format!("{}/{:010}.json", events_path, i);
        let serialized = serde_json_core::to_vec::<_, 2048>(&orphan).unwrap();
        storage.write_file(&event_file_path, &serialized).await.unwrap();
    }

    // -- Act: create a new store (restart) --
    let recovered_store = OpossumStore::new(Arc::clone(&storage), FakeClock::new(1000));

    let post_events: Vec<EventData> = (1..=2)
        .map(|i| EventData {
            event_id: uuid::Uuid::new_v4().to_string(),
            event: DomainEvent {
                event_type: "PostRecoveryEvent".to_string(),
                data: serde_json::to_string(&PostRecoveryEvent { data: format!("post-{}", i) }).unwrap(),
                tags: vec![],
            },
            metadata: None,
        })
        .collect();

    recovered_store.append_async(post_events, None).await.unwrap();

    // -- Assert: ReadAsync returns all 7 events in ascending position order --
    let all_events = recovered_store.read_async(Query::all(), None, None, None).await.unwrap();

    assert_eq!(all_events.len(), 7);
    for i in 0..all_events.len() {
        
        
        assert_eq!(all_events[i].position, i as u64);
    }

    for i in 0..3 {
        let data: OrphanedEvent = serde_json::from_str(&all_events[i].event.data.replace("\\\"", "\"").trim_matches('"')).unwrap();
        assert_eq!(data.data, format!("normal-{}", i + 1));
    }

    for i in 3..5 {
        let data: OrphanedEvent = serde_json::from_str(&all_events[i].event.data.replace("\\\"", "\"").trim_matches('"')).unwrap();
        assert_eq!(data.data, format!("orphaned-{}", i));
    }

    for i in 5..7 {
        let data: PostRecoveryEvent = serde_json::from_str(&all_events[i].event.data.replace("\\\"", "\"").trim_matches('"')).unwrap();
        assert_eq!(data.data, format!("post-{}", i - 4));
    }
}
