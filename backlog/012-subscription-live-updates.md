# Live Event Subscriptions

## Status: Not Started

## Description
Add ability to subscribe to new events in real-time instead of polling.

## Acceptance Criteria
- [ ] `subscribe(query: Query)` returns async stream of events
- [ ] Catches up from last position then switches to live
- [ ] Backpressure handling
- [ ] Multiple concurrent subscriptions
- [ ] Graceful shutdown

## API Design
```rust
impl EventStore {
    pub fn subscribe(&self, query: Query, from_position: Option<u64>) 
        -> impl Stream<Item = Result<EventRecord, Error>>;
}

// Or with callback
impl EventStore {
    pub async fn subscribe<F>(&self, query: Query, from_position: Option<u64>, handler: F)
    where F: Fn(EventRecord) -> impl Future<Output = ()>;
}
```

## Implementation Options
- File system watching (inotify/FSEvents)
- Polling with configurable interval
- Channel-based for in-memory notifications

## Dependencies
- EventStore (done)
- Event indexing (backlog/004) - for efficient filtering
