# Dynamic Consistency Boundary (DCB) Specification (Rust Mapping)

This document defines the minimal Event Store capabilities needed for DCB-style consistency checks,
mapped to the current `marmosa` API shape.

It is intentionally focused on behavior, not on exact type or method names.

## Required Event Store Capabilities

An Event Store is DCB-capable when it can:

- Read sequenced events using a query filter.
- Optionally read from a given sequence position onward.
- Atomically append one or more events.
- Reject appends when a consistency condition is violated.

In `marmosa`, this maps to `EventStore`:

```rust
use marmosa::domain::{AppendCondition, EventData, EventRecord, Query, ReadOption};
use marmosa::ports::Error;

trait EventStoreLike {
    fn append_async(
        &self,
        events: Vec<EventData>,
        condition: Option<AppendCondition>,
    ) -> impl core::future::Future<Output = Result<(), Error>> + Send;

    fn read_async(
        &self,
        query: Query,
        start_position: Option<u64>,
        max_count: Option<usize>,
        options: Option<Vec<ReadOption>>,
    ) -> impl core::future::Future<Output = Result<Vec<EventRecord>, Error>> + Send;
}
```

## Read Semantics

A compliant implementation should support:

- Filtering by event type and tags.
- Reading from a position (exclusive in `marmosa` runtime behavior).
- Optional limit and ordering controls.

`Query` matching is defined as:

- Query items are combined with **OR**.
- Within one query item:
  - `event_types` are matched with **OR**.
  - `tags` are matched with **AND**.

```rust
use marmosa::domain::{Query, QueryItem, Tag};

let query = Query {
    items: vec![
        QueryItem {
            event_types: vec!["CourseDefined".to_string(), "CourseRenamed".to_string()],
            tags: vec![],
        },
        QueryItem {
            event_types: vec!["CapacityChanged".to_string()],
            tags: vec![
                Tag { key: "courseId".to_string(), value: "c1".to_string() },
                Tag { key: "tenant".to_string(), value: "t1".to_string() },
            ],
        },
    ],
};
```

## Write Semantics

Appending events should be atomic for the submitted batch.

If `AppendCondition` is present, append must fail when at least one event matches
`fail_if_events_match`, considering `after_sequence_position` as the lower bound for
condition evaluation.

`marmosa` domain mapping:

```rust
use marmosa::domain::{AppendCondition, Query};

let condition = AppendCondition {
    fail_if_events_match: Query::all(),
    after_sequence_position: Some(42),
};
```

## Core Concepts (Mapped)

- **Sequenced Event**: `EventRecord` (`event` + `position` + metadata/timestamp fields).
- **Sequence Position**: `u64` `position` assigned at append time; unique and monotonic.
- **Event**: `DomainEvent` inside `EventData`/`EventRecord`.
- **Query**: `Query { items: Vec<QueryItem> }`, where `Query::all()` is the match-all variant.
- **Tags**: `Vec<Tag { key, value }>` used as domain-level partitioning/filter metadata.
- **Append Condition**: `AppendCondition { fail_if_events_match, after_sequence_position }`.

## DCB Decision Safety Pattern

The common DCB flow is:

1. Read only relevant events.
2. Build a decision model state.
3. Append new events with an `AppendCondition` derived from the read boundary.

In this repository, `decision_model::build_decision_model_from_events` and
`decision_model::build_decision_model_nary_from_events` produce both state and append condition
material aligned to that pattern.

```rust,ignore
use marmosa::decision_model::{build_decision_model_from_events, DelegateDecisionProjection};
use marmosa::domain::Query;

// projection + relevant events obtained from the store
let model = build_decision_model_from_events(&projection, &events);

// use model.append_condition when appending command results
store.append_async(new_events, Some(model.append_condition)).await?;
```

## Notes

- API names may evolve; DCB compliance is defined by behavior.
- Implementations may include additional read/write options without affecting DCB semantics.
- Query narrowing is a correctness and performance concern: narrower queries create narrower
  dynamic consistency boundaries.
