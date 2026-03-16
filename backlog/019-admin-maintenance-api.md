# Event Store Admin & Maintenance API

## Status: Not Started

## Description
Opossum exposes management APIs via `EventStoreAdminTests.cs` and `EventStoreMaintenanceTests.cs`. These include inspecting the raw store, verifying sizes, pruning historical snapshots, and truncating.

## Acceptance Criteria
- [ ] Implement `EventStoreAdmin` trait or methods.
- [ ] Ability to query total size and number of events (`get_statistics()`).
- [ ] Support for soft-deleting and hard-deleting event ranges / truncating streams.
- [ ] Support for index rebuilding (if the tag index becomes corrupted).

## API Design
```rust
pub trait EventStoreAdmin {
    async fn truncate(&self, before_position: u64) -> Result<(), AdminError>;
    async fn get_statistics(&self) -> Result<StoreStatistics, AdminError>;
    async fn rebuild_indices(&self) -> Result<(), AdminError>;
}
```