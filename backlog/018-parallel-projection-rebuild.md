# Parallel Projection Rebuilding

## Status: Not Started

## Description
Based on `Projections/ParallelRebuildTests.cs` and `ParallelRebuildLockingTests.cs`.
Currently, `ProjectionRunner` runs synchronously or relies on `tokio::join!` implicitly. A dedicated subsystem for completely rebuilding large projections across multiple threads concurrently is required.

## Acceptance Criteria
- [ ] Implement `ParallelProjectionManager` that chunks rebuilding work by subsets or segments.
- [ ] Ensure safe coordination in `tokio` to parallelize apply operations using channel workers (`mpsc`/`Semaphore`).
- [ ] Provide locking structures so that `rebuild_async` doesn't conflict with active live writes.

## API Design
```rust
pub struct BuildOptions {
    pub max_concurrent_workers: usize,
    pub batch_size: usize,
}
pub trait ParallelProjectionRunner {
    async fn rebuild_parallel(&self, defs: &[Box<dyn ProjectionDefinition>], options: BuildOptions) -> Result<(), Error>;
}
```