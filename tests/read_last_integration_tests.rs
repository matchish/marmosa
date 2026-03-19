mod common;

use common::{FakeClock, InMemoryStorage};
use marmosa::domain::{AppendCondition, Query, QueryItem};
use marmosa::event_store::MarmosaStore;
use marmosa::extensions::{EventStoreExt, ToDomainEventExt};
use marmosa::ports::Error;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::task;

#[derive(Serialize, Deserialize, Debug, Clone)]
struct InvoiceCreatedEvent {
    invoice_number: i32,
}

fn create_store() -> Arc<MarmosaStore<Arc<InMemoryStorage>, FakeClock>> {
    let storage = Arc::new(InMemoryStorage::new());
    let clock = FakeClock::new(1000);
    Arc::new(MarmosaStore::new(storage, clock))
}

fn invoice_query() -> Query {
    Query {
        items: vec![QueryItem {
            event_types: vec!["InvoiceCreatedEvent".to_string()],
            tags: vec![],
        }],
    }
}

fn parse_invoice(data: &str) -> InvoiceCreatedEvent {
    let clean = data.replace("\\\"", "\"");
    serde_json::from_str(&clean).unwrap()
}

async fn create_next_invoice_async(
    store: Arc<MarmosaStore<Arc<InMemoryStorage>, FakeClock>>,
) -> i32 {
    let query = invoice_query();

    // retry manually logic
    for _ in 0..20 {
        let last = store.read_last_async(query.clone()).await.unwrap();

        let (next_number, after_sequence_position) = if let Some(last_event) = last {
            let data = parse_invoice(&last_event.event.data);
            (data.invoice_number + 1, Some(last_event.position))
        } else {
            (1, None)
        };

        let condition = AppendCondition {
            fail_if_events_match: query.clone(),
            after_sequence_position,
        };

        let event = InvoiceCreatedEvent {
            invoice_number: next_number,
        };
        // UNIQUE EVENT IDs to avoid identical event id failures!
        // In real concurrent scenarios, EventID should be random.
        let event_data =
            event
                .to_domain_event()
                .build(&format!("inv-{}-{}", next_number, uuid::Uuid::new_v4()));

        match store.append_single_async(event_data, Some(condition)).await {
            Ok(_) => return next_number,
            Err(Error::AppendConditionFailed) => continue,
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    panic!("Failed to append after 20 retries");
}

#[tokio::test]
async fn read_last_async_with_no_invoices_returns_null_async() {
    let store = create_store();
    let result = store.read_last_async(invoice_query()).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn read_last_async_after_first_invoice_returns_it_async() {
    let store = create_store();
    let event = InvoiceCreatedEvent { invoice_number: 1 };
    store
        .append_single_async(event.to_domain_event().build("inv-1"), None)
        .await
        .unwrap();

    let result = store.read_last_async(invoice_query()).await.unwrap();
    assert!(result.is_some());

    let result = result.unwrap();
    let data = parse_invoice(&result.event.data);
    assert_eq!(data.invoice_number, 1);
}

#[tokio::test]
async fn read_last_async_after_multiple_invoices_returns_highest_position_async() {
    let store = create_store();
    for i in 1..=3 {
        let event = InvoiceCreatedEvent { invoice_number: i };
        store
            .append_single_async(event.to_domain_event().build(&format!("inv-{}", i)), None)
            .await
            .unwrap();
    }

    let result = store.read_last_async(invoice_query()).await.unwrap();
    assert!(result.is_some());

    let result = result.unwrap();
    let data = parse_invoice(&result.event.data);
    assert_eq!(data.invoice_number, 3);
    assert_eq!(result.position, 2); // 3 items = positions 0, 1, 2
}

#[tokio::test]
async fn read_last_async_with_interleaved_event_types_returns_only_last_match_async() {
    let store = create_store();

    // Append invoice 1
    let event1 = InvoiceCreatedEvent { invoice_number: 1 };
    store
        .append_single_async(event1.to_domain_event().build("inv-1"), None)
        .await
        .unwrap();

    // Append something else to interleave
    #[derive(Serialize)]
    struct OtherEvent {
        val: i32,
    }
    let other = OtherEvent { val: 42 };
    store
        .append_single_async(other.to_domain_event().build("other-1"), None)
        .await
        .unwrap();

    // Append invoice 2
    let event2 = InvoiceCreatedEvent { invoice_number: 2 };
    store
        .append_single_async(event2.to_domain_event().build("inv-2"), None)
        .await
        .unwrap();

    let result = store.read_last_async(invoice_query()).await.unwrap();
    assert!(result.is_some());

    let result = result.unwrap();
    let data = parse_invoice(&result.event.data);
    assert_eq!(data.invoice_number, 2);
    assert_eq!(result.position, 2); // items at 0, 1, 2. the 3rd item is at position 2
}

#[tokio::test]
async fn invoice_numbering_first_invoice_gets_number_one_async() {
    let store = create_store();
    let number = create_next_invoice_async(store.clone()).await;
    assert_eq!(number, 1);
}

#[tokio::test]
async fn invoice_numbering_sequential_creation_forms_continuous_sequence_async() {
    let store = create_store();
    let count = 5;

    for _ in 0..count {
        create_next_invoice_async(store.clone()).await;
    }

    let events = store.read_all_async(invoice_query()).await.unwrap();
    let mut numbers: Vec<i32> = events
        .into_iter()
        .map(|e| {
            let data = parse_invoice(&e.event.data);
            data.invoice_number
        })
        .collect();
    numbers.sort();

    assert_eq!(numbers.len(), count as usize);
    for i in 0..count {
        assert_eq!(numbers[i as usize], i + 1);
    }
}

#[tokio::test]
async fn invoice_numbering_concurrent_creation_produces_unique_sequential_numbers_without_gaps_async()
 {
    let store = create_store();
    let concurrent_writers = 20;

    let mut handles = Vec::new();
    for _ in 0..concurrent_writers {
        let store_clone = store.clone();
        handles.push(task::spawn(async move {
            create_next_invoice_async(store_clone).await
        }));
    }

    let mut invoice_numbers = Vec::new();
    for h in handles {
        let num = h.await.unwrap();
        invoice_numbers.push(num);
    }

    assert_eq!(invoice_numbers.len(), concurrent_writers);
    invoice_numbers.sort();

    for i in 0..concurrent_writers {
        assert_eq!(invoice_numbers[i], (i + 1) as i32);
    }
}

#[tokio::test]
async fn invoice_numbering_append_condition_rejects_stale_decision_async() {
    let store = create_store();
    let query = invoice_query();

    // Read: no invoices yet
    let last = store.read_last_async(query.clone()).await.unwrap();
    assert!(last.is_none());

    // Simulate another writer appending invoice #1
    let event = InvoiceCreatedEvent { invoice_number: 1 };
    store
        .append_single_async(event.to_domain_event().build("inv-1"), None)
        .await
        .unwrap();

    // Our condition says "no invoices should exist"
    let condition = AppendCondition {
        fail_if_events_match: query.clone(),
        after_sequence_position: None,
    };

    let event2 = InvoiceCreatedEvent { invoice_number: 1 };
    let err = store
        .append_single_async(
            event2.to_domain_event().build("inv-duplicate"),
            Some(condition),
        )
        .await;

    assert_eq!(err, Err(Error::AppendConditionFailed));
}
