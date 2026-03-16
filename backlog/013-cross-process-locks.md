# Cross-Process Safety & Locking

## Status: Not Started

## Description
Based on `CrossProcessAppendSafetyTests.cs` and `CrossProcessLockManagerTests.cs`, multiple instances of Marmosa operating on the same directory MUST ensure data integrity without corruption using file-system locks.

## Acceptance Criteria
- [ ] Implement a file-level locking mechanism for appending to the event stream (`std::fs::File::lock` or `fs2` equivalent).
- [ ] Handle lock contention timeouts and retry logic.
- [ ] Support cooperative or exclusive locking for reader/writer optimization.
- [ ] Implement tests spawning separate OS processes to verify concurrent appends don't clobber events.

## API Design
```rust
pub trait StorageBackend {
    async fn acquire_exclusive_lock(&self, resource: &str, timeout_ms: u64) -> Result<LockGuard, StorageError>;
}
```