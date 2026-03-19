# Projection Retention Policies (Design Guide)

## Overview

Retention policies define projection lifecycle behavior over time:

- keep forever
- delete after a retention window
- archive after a retention window

This helps control storage growth, satisfy deletion requirements, and keep projection directories manageable.

## Current Status

This guide is migrated from the C# source specification.

The crate does not yet expose a finalized retention-policy API, so this document is implementation guidance for future work.

## Suggested Rust Model

Use explicit enums and structs:

```rust,ignore
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetentionPolicyType {
    NoRetention,
    DeleteAfter,
    ArchiveAfter,
}

#[derive(Debug, Clone)]
pub struct ProjectionRetentionPolicy {
    pub policy_type: RetentionPolicyType,
    pub retain_for_ms: u64,
}
```

Prefer explicit `NoRetention` as the default to make intent clear.

## Tombstone Concept

When a projection is deleted or archived by policy, write a tombstone marker to prevent immediate rebuild loops.

Suggested tombstone payload includes:

- deletion timestamp
- deletion reason
- optional last known metadata
- optional archive location
- policy type that triggered deletion

## Policy Execution Flow

1. Resolve policy per projection (specific override or global default)
2. Skip if policy is `NoRetention`
3. Compute cutoff timestamp (`now - retain_for_ms`)
4. Query candidates from metadata index (`last_updated_at < cutoff`)
5. Apply delete/archive action per candidate
6. Write audit log and tombstone

## Existing Building Blocks in This Crate

Retention implementation can build on existing APIs:

- `ProjectionMetadata`
- `ProjectionMetadataIndex::get_updated_since` (inverse logic for expiration checks)
- `ProjectionStore::delete`

## Safety & Observability Rules

- Never crash application startup/background loops due to retention errors.
- Log all retention actions (projection key, action type, reason).
- Process each projection key atomically where possible.
- Keep manual execution available for operators.

## Example API Sketch

```rust,ignore
pub trait ProjectionRetentionService {
    async fn execute_policy(&self, projection_name: &str) -> Result<(), RetentionError>;
    async fn execute_all_policies(&self) -> Result<(), RetentionError>;
}
```

Use `Result`-based error handling and avoid hidden panics during policy execution.
