# DCB Projections (Rust Guide)

This guide describes DCB-oriented projection patterns in Rust terms, using the primitives available
in `marmosa`.

## Why Projections Matter in DCB

Event stores keep history; decision logic usually needs current, derived state.
DCB projections rebuild exactly the state needed to validate invariants before append.

The design goal is to keep the decision scope narrow:

- load only relevant events,
- derive only required state,
- guard append with a matching consistency condition.

## Projection as a Fold

Conceptually, a projection is a pure reducer:

```rust
type Projection<S, E> = fn(S, &E) -> S;
```

In this crate, the decision-side equivalent is `DecisionProjection`:

- `initial_state()`
- `query()`
- `apply(state, event)`

```rust
use marmosa::decision_model::{DecisionProjection, DelegateDecisionProjection};
use marmosa::domain::{EventRecord, Query};

let projection = DelegateDecisionProjection::new(false, Query::all(), |state, _event: &EventRecord| {
    state
});

assert!(!projection.initial_state());
```

## Filter Before Rebuild

The projection query should represent only the events needed by the invariant.

- Event type filters narrow by behavior.
- Tag filters narrow by entity/tenant/context.

```rust
use marmosa::domain::{Query, QueryItem, Tag};

fn course_exists_query(course_id: &str) -> Query {
    Query {
        items: vec![QueryItem {
            event_types: vec!["CourseDefined".to_string(), "CourseArchived".to_string()],
            tags: vec![Tag {
                key: "courseId".to_string(),
                value: course_id.to_string(),
            }],
        }],
    }
}
```

## Factory Pattern for Dynamic Projections

Because identifiers are runtime data, use factory functions:

```rust
use marmosa::decision_model::DelegateDecisionProjection;
use marmosa::domain::{EventRecord, Query, QueryItem, Tag};

fn course_exists_projection(course_id: &str) -> DelegateDecisionProjection<bool, impl Fn(bool, &EventRecord) -> bool> {
    let query = Query {
        items: vec![QueryItem {
            event_types: vec!["CourseDefined".to_string(), "CourseArchived".to_string()],
            tags: vec![Tag {
                key: "courseId".to_string(),
                value: course_id.to_string(),
            }],
        }],
    };

    DelegateDecisionProjection::new(false, query, |state, event| match event.event.event_type.as_str() {
        "CourseDefined" => true,
        "CourseArchived" => false,
        _ => state,
    })
}
```

## Compose Small Projections

Prefer multiple small projections over one large projection. Small projections are:

- easier to reason about,
- easier to test,
- less greedy in query scope.

`build_decision_model_nary_from_events` already supports multi-projection decision construction and
returns an append condition based on the union of projection queries.

```rust,ignore
use marmosa::decision_model::build_decision_model_nary_from_events;

let projections = vec![&course_exists_projection("c1"), &capacity_projection("c1")];
let (states, append_condition) = build_decision_model_nary_from_events(&projections, &events);

store.append_async(new_events, Some(append_condition)).await?;
```

## End-to-End DCB Shape

Typical command flow:

1. Build projection(s) for the command target.
2. Read matching events from store with projection query.
3. Fold into decision state.
4. Validate invariants.
5. Append resulting events with the produced append condition.

This ties writes to the exact read boundary and rejects stale concurrent decisions.

## Practical Guidelines

- Keep projection handlers pure and deterministic.
- Keep query tags explicit (e.g., `courseId`, `tenantId`).
- Prefer composition for independent constraints.
- Use narrow event types and tags to minimize boundary width.
- Reuse decision projection factories across command handlers.
