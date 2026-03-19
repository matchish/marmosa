# Smart Retention (Design Guide)

## Overview

Smart retention extends simple time-based cleanup by considering both:

- data age
- observed access behavior

This avoids deleting old-but-hot data while still archiving or deleting cold data.

## Current Status

This is a migrated design guide from the C# source specs.

The crate currently does not expose a finalized smart-retention API, so this document is implementation guidance.

## Suggested Rust Model

Prefer explicit enums and policy structs:

```rust,ignore
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetentionPolicyKind {
    NoRetention,
    DeleteAfter,
    ArchiveAfter,
    SmartRetention,
}

#[derive(Debug, Clone)]
pub struct SmartRetentionPolicy {
    pub minimum_age_ms: u64,
    pub inactivity_threshold_ms: u64,
}
```

Optional access tracking config:

```rust,ignore
#[derive(Debug, Clone)]
pub struct AccessTrackingConfig {
    pub enabled: bool,
    pub update_granularity_ms: u64,
    pub batch_updates: bool,
    pub batch_flush_interval_ms: u64,
}
```

## Candidate Evaluation

A projection is a smart-retention candidate when both conditions hold:

1. `now - last_updated_at >= minimum_age`
2. `now - last_accessed_at >= inactivity_threshold`

If access tracking is disabled, smart-retention should be unavailable or explicitly downgraded to time-based behavior.

## Existing Building Blocks in This Crate

Current APIs useful for a future implementation:

- `ProjectionMetadata` (already tracks `created_at`, `last_updated_at`, `size_in_bytes`)
- `ProjectionMetadataIndex` (`get_all`, `get_updated_since`)
- projection delete flow in `ProjectionStore::delete`

Future smart-retention likely needs additional metadata fields, for example:

- `last_accessed_at`
- `access_count`

## Lightweight Access Tracking Guidance

- Keep tracking opt-in (`enabled = false` by default).
- Avoid write-on-every-read behavior.
- Batch and coalesce updates by key.
- Persist updates asynchronously with bounded flush intervals.

## Safety Rules

- Never block reads on access-tracking writes.
- Never lose projection data on retention decision failure.
- Keep retention actions auditable (key, reason, policy, timestamp).
- Make manual execution available for operators.

## Optional Statistics Surface

```rust,ignore
pub trait ProjectionAccessStatistics {
    async fn most_accessed(&self, projection_name: &str, count: usize)
        -> Result<Vec<ProjectionAccessInfo>, StatsError>;

    async fn least_recently_accessed(&self, projection_name: &str, count: usize)
        -> Result<Vec<ProjectionAccessInfo>, StatsError>;
}
```

This can support cache-warming prioritization and operator diagnostics.
