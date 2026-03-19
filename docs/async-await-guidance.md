# Async/Await Guidance for Marmosa

This guide replaces the old `.NET ConfigureAwait(false)` checklist with Rust-native async guidance.

## Core Rule

Library async code should remain runtime-agnostic and avoid assuming a specific executor beyond
the crate's public API contracts.

## Practical Rules

- Keep public async APIs executor-neutral where possible.
- Require `Send + Sync` on traits and futures when cross-thread execution is expected.
- Avoid blocking calls inside async paths (`std::thread::sleep`, sync filesystem/network I/O).
- Use `spawn_blocking` (or equivalent) for unavoidable blocking work.
- Keep cancellation-safe behavior for long-running loops.

## Patterns

```rust
use core::future::Future;

pub trait ProjectionStore<TState> {
    fn get(
        &self,
        key: &str,
    ) -> impl Future<Output = Result<Option<TState>, marmosa::ports::Error>> + Send;
}
```

```rust,no_run
// Example shape: offload blocking work from async context
async fn expensive_sync_work() {
    let _result = tokio::task::spawn_blocking(|| {
        // CPU-heavy or blocking operation
        42
    })
    .await;
}
```

## Review Checklist

- [ ] No accidental blocking calls in async code paths
- [ ] `Send` bounds are present where concurrency requires them
- [ ] Trait/object safety decisions are explicit
- [ ] Tests cover concurrent behavior and cancellation-sensitive paths
