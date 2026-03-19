# Cache Warming (Design Guide)

## Overview

Cache warming is an opt-in startup strategy that pre-reads selected files so first user requests avoid cold disk reads.

In this crate, the idea fits projection-heavy workloads where predictable request latency is more important than minimal startup time.

Typical trade-off:

- Faster first requests after startup
- Slower startup due to preloading work

## Current Status

This is a migrated design guide from the C# source project.

The crate currently does **not** expose a finalized cache-warming API in public modules. Treat this document as implementation guidance for future work.

## Suggested Rust Model

Use crate terms and Rust-friendly boundaries:

- `CacheWarmingOptions` as a configuration struct
- `CacheWarmer` as a service object
- A `WarmupBudget` value object to enforce safety limits

```rust,ignore
#[derive(Debug, Clone)]
pub struct CacheWarmingOptions {
    pub enabled: bool,
    pub warm_projections_since_ms: Option<u64>,
    pub warm_events_since_ms: Option<u64>,
    pub max_warmup_duration_ms: u64,
    pub max_files_to_warm: usize,
    pub max_warmup_size_bytes: u64,
    pub always_warm_indices: bool,
}
```

## Warmup Priority

Recommended order:

1. Projection tag/type indices
2. Projection metadata index
3. Recently updated projection files
4. Recent event files (optional)

This order maximizes value per byte read.

## Budget & Safety Rules

Warmup should stop when any budget is exhausted:

- File-count limit reached
- Size budget reached
- Duration timeout reached

Warmup failures should be logged and skipped, not crash startup.

## Integration Notes

- Run warming before serving requests in applications that need stable first-request latency.
- Keep the feature opt-in (`enabled = false` by default).
- Log: warmed file count, warmed bytes, and elapsed time.

## Relationship to Existing Projection APIs

This design builds on existing projection metadata and index concepts in:

- `ProjectionMetadata`
- `ProjectionMetadataIndex`
- projection tag indices

When implemented, use metadata timestamps (`last_updated_at`) to choose recent projection candidates.
