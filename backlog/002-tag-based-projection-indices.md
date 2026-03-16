# Tag-Based Projection Indices

## Status: Not Started

## Description
Add indexing support for projections so they can be queried efficiently by tags without loading all projections into memory.

## Acceptance Criteria
- [ ] `IProjectionTagProvider` trait for extracting tags from projection state
- [ ] Tag indices stored at `Projections/{name}/Indices/{tag_key}/{tag_value}/`
- [ ] `query_by_tag(tag: Tag)` method on ProjectionStore
- [ ] `query_by_tags(tags: Vec<Tag>)` with AND logic
- [ ] Indices updated on save/delete

## API Design
```rust
pub trait ProjectionTagProvider<TState> {
    fn get_tags(&self, state: &TState) -> Vec<Tag>;
}

// On ProjectionStore
async fn query_by_tag(&self, tag: Tag) -> Result<Vec<TState>, Error>;
async fn query_by_tags(&self, tags: &[Tag]) -> Result<Vec<TState>, Error>;
```

## Dependencies
- StorageBackendProjectionStore (done)
