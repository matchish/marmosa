# Projection Rebuild Support

## Status: Not Started

## Description
Add ability to rebuild projections from scratch by replaying all events. This is needed when projection logic changes or data becomes corrupted.

## Acceptance Criteria
- [ ] `rebuild()` method on ProjectionRunner
- [ ] Clears existing projection state before rebuilding
- [ ] Resets checkpoint to 0
- [ ] Processes all events from the beginning
- [ ] Atomic rebuild (readers see old data until rebuild completes)
- [ ] Optional: parallel rebuild for large event stores

## API Design
```rust
impl ProjectionRunner {
    /// Rebuild projection from scratch
    pub async fn rebuild(&self) -> Result<u64, Error>;
    
    /// Rebuild with progress callback
    pub async fn rebuild_with_progress<F>(&self, on_progress: F) -> Result<u64, Error>
    where F: Fn(u64, u64); // (processed, total)
}
```

## Considerations
- Temp directory approach (like Opossum) for atomic swaps
- Memory usage for large rebuilds
- Progress reporting for long rebuilds

## Dependencies
- ProjectionRunner (done)
- EventStore read_async (done)
