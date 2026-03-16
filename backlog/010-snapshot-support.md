# Aggregate Snapshot Support

## Status: Not Started

## Description
Add snapshot support for aggregates to avoid replaying all events when loading aggregate state.

## Acceptance Criteria
- [ ] Snapshot storage alongside events
- [ ] Configurable snapshot frequency (every N events)
- [ ] Load from snapshot + replay subsequent events
- [ ] Snapshot invalidation on schema change

## API Design
```rust
pub trait Aggregate {
    type State;
    type Event;
    
    fn apply(&self, state: &mut Self::State, event: &Self::Event);
    fn snapshot_interval(&self) -> Option<u64> { Some(100) }
}

pub struct AggregateRepository<A: Aggregate> {
    event_store: EventStore,
    snapshot_store: SnapshotStore,
}

impl<A: Aggregate> AggregateRepository<A> {
    pub async fn load(&self, id: &str) -> Result<A::State, Error>;
    pub async fn save(&self, id: &str, events: Vec<A::Event>) -> Result<(), Error>;
}
```

## Dependencies
- EventStore (done)
- StorageBackend (done)
