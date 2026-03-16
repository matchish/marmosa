# Event Store Crash Recovery

## Status: Not Started

## Description
Based on Opossum's `CrashRecoveryTests.cs` and `ProjectionRebuildCrashRecoveryTests.cs`, the file-based event store needs mechanisms to recover from unsafe shutdowns or power failures, such as partial writes or orphaned lock files.

## Acceptance Criteria
- [ ] Implement startup integrity checks for the event ledger.
- [ ] Detect and handle partially written `.json` files.
- [ ] Recover from interrupted projection rebuilds (resume via checkpoints).
- [ ] `EventStoreMaintenance.repair_ledger()` API to physically remove corrupted trailing bytes/files.
- [ ] Integration tests simulating a crash (e.g. abrupt termination mid-write).

## API Design
```rust
pub trait StorageBackend {
    // ...
    async fn verify_integrity(&self) -> Result<IntegrityReport, StorageError>;
    async fn repair(&self) -> Result<(), StorageError>;
}
```