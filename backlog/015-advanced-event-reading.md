# Advanced Event Reading Queries

## Status: Not Started

## Description
Based on `ReadLastIntegrationTests.cs`, `DescendingPerformanceTests.cs`, and `ReadFromPositionIntegrationTests.cs`.
Currently, `OpossumStore::read` simply retrieves all events and filters. It needs the ability to efficiently read segments, read backward (descending), and read only the latest event.

## Acceptance Criteria
- [ ] Implement `EventStore::read_backwards(query, limit)`.
- [ ] Implement `EventStore::read_from_position(position, direction)`.
- [ ] Implement optimized `EventStore::read_last(query)` to avoid full scans.
- [ ] The tag index must support reverse iteration and binary search for bounds.

## API Design
```rust
pub trait EventStore {
    async fn read_last(&self, query: &Query) -> Result<Option<EventRecord>, StoreError>;
    async fn read_backwards(&self, query: &Query, limit: Option<usize>) -> Result<Vec<EventRecord>, StoreError>;
    async fn read_from_position(&self, position: u64, limit: Option<usize>) -> Result<Vec<EventRecord>, StoreError>;
}
```