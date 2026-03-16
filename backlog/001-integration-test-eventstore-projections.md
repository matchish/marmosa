# Integration Test: EventStore + Projections End-to-End

## Status: Done

## Description
Wire up `OpossumStore` and `ProjectionRunner` in a single integration test to verify the full event sourcing + projection flow works correctly.

## Acceptance Criteria
- [x] Append events to the event store
- [x] ProjectionRunner reads events from event store
- [x] Projection state is correctly materialized
- [x] Checkpoint is saved and respected on restart
- [x] Multiple projection instances (different keys) work correctly

## Test Scenario
```rust
// 1. Create event store and projection runner
// 2. Append CounterIncremented events for multiple counters
// 3. Run projection
// 4. Verify each counter has correct count
// 5. Append more events
// 6. Run projection again (should resume from checkpoint)
// 7. Verify counts updated correctly
```

## Dependencies
- EventStore (done)
- ProjectionRunner (done)
- StorageBackendProjectionStore (done)
