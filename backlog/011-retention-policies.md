# Event Retention Policies

## Status: Not Started

## Description
Add support for event retention policies to manage storage growth over time.

## Acceptance Criteria
- [ ] Time-based retention (delete events older than X days)
- [ ] Count-based retention (keep only last N events per tag)
- [ ] Archive before delete option
- [ ] Projection-aware deletion (don't delete if projection needs it)

## API Design
```rust
pub struct RetentionPolicy {
    pub max_age: Option<Duration>,
    pub max_count_per_tag: Option<usize>,
    pub archive_before_delete: bool,
}

impl EventStore {
    pub async fn apply_retention(&self, policy: &RetentionPolicy) -> Result<RetentionStats, Error>;
}
```

## Considerations
- Must not break projection checkpoints
- Tombstones for deleted events?
- Archive format and location

## Dependencies
- EventStore (done)
- Event indexing (backlog/004)
