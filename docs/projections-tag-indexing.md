# Projection Tag Indexing

Tag-based indexing for projections enables efficient querying without loading all projection instances into memory. This guide explains how tag indices work, how to use them, and best practices for implementing tag providers.

## Overview

Tag-based indexing allows you to filter projections by tagged attributes, similar to how events are queried by tags. Instead of loading all projections and filtering in memory, queries use pre-built indices for O(1) lookups.

### Problem Solved

#### Without tag indexing
```rust
// ❌ Loads ALL student projections into memory
let all_students = store.get_all_async().await?;
let premium_students: Vec<_> = all_students
    .into_iter()
    .filter(|s| s.enrollment_tier == EnrollmentTier::Premium)
    .collect();
```

#### With tag indexing
```rust
// ✅ Only loads Premium student projections from index
use marmosa::domain::Tag;

let premium_students = store
    .query_by_tag_async(Tag {
        key: "EnrollmentTier".to_string(),
        value: "Premium".to_string(),
    })
    .await?;
```

## on-Disk Structure

Indices are stored per projection type in subdirectories:

```
{root_path}/Projections/
  StudentShortInfo/
    keys.json                    ← Projection keys
    Indices/                     ← Tag indices directory
      EnrollmentTier_Basic.json   ← ["student-id-1", "student-id-2", ...]
      EnrollmentTier_Premium.json
      EnrollmentTier_Enterprise.json
      IsMaxedOut_true.json
      IsMaxedOut_false.json
  CourseShortInfo/
    keys.json
    Indices/
      IsFull_true.json
      IsFull_false.json
```

## Architecture

### Core Components

1. **`ProjectionTagProvider` trait**
   - Extracts tags from a projection state
   - Implemented per projection type
   - You control which properties become indexed tags

2. **`ProjectionTagIndex<S>`**
   - Manages index files on disk
   - Thread-safe tag index operations
   - Automatically updates when projections are saved/deleted

3. **`ProjectionMetadata` struct**
   - Tracks version, size, and timestamps
   - Stored alongside projection data

4. **Query methods on `ProjectionStore<S>`**
   - `query_by_tag_async(tag)` — single tag filter
   - `query_by_tags_async(tags)` — multiple tags (AND logic)

### Tag Lifecycle

#### When a projection is saved

```rust
await store.save_async(key, projection)?;

// Internally:
// 1. Extract tags using ProjectionTagProvider::get_tags()
// 2. Load old tags from metadata (if projection exists)
// 3. Remove key from old tag indices
// 4. Add key to new tag indices
// 5. Persist the projection state and metadata
```

#### When a projection is updated

```rust
// Tier changes from Basic → Premium
projection.enrollment_tier = EnrollmentTier::Premium;
await store.save_async(key, &projection)?;

// Internally:
// 1. Remove key from "EnrollmentTier_Basic.json"
// 2. Add key to "EnrollmentTier_Premium.json"
// 3. Save updated projection
```

#### When a projection is deleted

```rust
await store.delete_async(key)?;

// Internally:
// 1. Remove key from all tag indices
// 2. Delete projection file
// 3. Update metadata index
```

## Usage Guide

### 1. Implement `ProjectionTagProvider`

Define which properties should be indexed by implementing the `ProjectionTagProvider` trait:

```rust
use marmosa::domain::Tag;
use marmosa::projections::ProjectionTagProvider;

pub struct StudentShortInfoTagProvider;

impl ProjectionTagProvider<StudentShortInfo> for StudentShortInfoTagProvider {
    fn get_tags(&self, state: &StudentShortInfo) -> Vec<Tag> {
        vec![
            // Index by enrollment tier
            Tag {
                key: "EnrollmentTier".to_string(),
                value: state.enrollment_tier.to_string(),
            },
            // Index by max-out status
            Tag {
                key: "IsMaxedOut".to_string(),
                value: state.is_maxed_out().to_string(),
            },
        ]
    }
}
```

### 2. Query Using Tags

#### Single Tag Query

```rust
use marmosa::domain::Tag;

let premium_students = store
    .query_by_tag_async(Tag {
        key: "EnrollmentTier".to_string(),
        value: "Premium".to_string(),
    })
    .await?;
```

#### Multi-Tag Query (AND logic)

```rust
let tags = vec![
    Tag {
        key: "EnrollmentTier".to_string(),
        value: "Premium".to_string(),
    },
    Tag {
        key: "IsMaxedOut".to_string(),
        value: "true".to_string(),
    },
];

let premium_maxed_out = store.query_by_tags_async(tags).await?;
```

#### Conditional Query

```rust
if let Some(filter) = request.tier_filter {
    let students = store
        .query_by_tag_async(Tag {
            key: "EnrollmentTier".to_string(),
            value: filter.to_string(),
        })
        .await?;
    Ok(students)
} else {
    // No filter—get all
    Ok(store.get_all_async().await?)
}
```

## Performance Characteristics

### Query Performance

| Operation | Without Tags | With Tags |
|-----------|------|-----------|
| Filter 10 from 10,000 | O(10,000) | O(10) |
| Filter 100 from 100,000 | O(100,000) | O(100) |
| Multi-tag filter | O(n) + filter | O(intersection) |

### Index Maintenance Costs

- **Save**: O(tags) — constant per tag
- **Update**: O(changed_tags) — only differences updated
- **Delete**: O(tags) — removes from all indices
- **Rebuild**: O(n × tags) — rebuilds all indices

## Best Practices

### 1. Choose Indexable Properties Wisely

✅ **Good candidates:**
- Enum values (Status, Tier, Category)
- Boolean flags (IsActive, IsFull, IsMaxedOut)
- Low-cardinality strings (Type, Region, Country)

❌ **Poor candidates:**
- High-cardinality values (Email, UUID, Name, Description)
- Frequently changing values
- Computed properties that change often

### 2. Tag Key Naming

Use clear, descriptive names:

```rust
// ✅ Clear and consistent
Tag { key: "EnrollmentTier".to_string(), value: "Premium".to_string() }
Tag { key: "IsMaxedOut".to_string(), value: "true".to_string() }

// ❌ Ambiguous
Tag { key: "tier".to_string(), value: "p" }
Tag { key: "status1".to_string(), value: "1" }
```

### 3. Tag Value Consistency

- Tag values are **case-sensitive** in storage and queries
- Use consistent casing (e.g., always `"Premium"`, never `"premium"`)
- Consider `to_string()` on enums for consistent formatting

### 4. Multi-Tag Queries

- Multiple tags use AND logic (intersection of results)
- The implementation optimizes by finding the smallest index first
- Queries are thread-safe

### 5. Error Handling

Tag extraction should fail fast on invalid state:

```rust
impl ProjectionTagProvider<StudentShortInfo> for StudentShortInfoTagProvider {
    fn get_tags(&self, state: &StudentShortInfo) -> Vec<Tag> {
        vec![
            Tag {
                key: "EnrollmentTier".to_string(),
                // This will panic if enrollment_tier is invalid
                // Better to fail during save than have inconsistent indices
                value: state.enrollment_tier.to_string(),
            },
        ]
    }
}
```

## Migration Guide

### Adding Tags to an Existing Projection

1. **Implement tag provider:**
   ```rust
   pub struct MyProjectionTagProvider;

   impl ProjectionTagProvider<MyProjection> for MyProjectionTagProvider {
       fn get_tags(&self, state: &MyProjection) -> Vec<Tag> {
           vec![Tag {
               key: "Status".to_string(),
               value: state.status.to_string(),
           }]
       }
   }
   ```

2. **Register the provider** (update your store initialization)

3. **Rebuild the projection:**
   ```bash
   # Delete projection folder to force rebuild
   rm -rf {root_path}/Projections/MyProjection

   # Restart application—indices will be rebuilt automatically
   ```

4. **Update query code:**
   ```rust
   // Before: load all and filter
   let all = store.get_all_async().await?;
   let active = all.iter().filter(|x| x.status == Status::Active).collect();

   // After: use tag index
   let active = store
       .query_by_tag_async(Tag {
           key: "Status".to_string(),
           value: "Active".to_string(),
       })
       .await?;
   ```

## Testing

### Unit Testing Tag Providers

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_provider_extracts_correct_tags() {
        let provider = StudentShortInfoTagProvider;
        let student = StudentShortInfo {
            student_id: "abc-123".to_string(),
            first_name: "Alice".to_string(),
            last_name: "Smith".to_string(),
            enrollment_tier: EnrollmentTier::Premium,
            current_enrollments: 5,
            max_enrollments: 5,
        };

        let tags = provider.get_tags(&student);

        assert_eq!(tags.len(), 2);
        assert!(tags.iter().any(|t| {
            t.key == "EnrollmentTier" && t.value == "Premium"
        }));
        assert!(tags.iter().any(|t| {
            t.key == "IsMaxedOut" && t.value == "true"
        }));
    }
}
```

### Integration Testing Tag Queries

Use temporary storage and actual projection lookups in tests:

```rust
#[tokio::test]
async fn tag_query_returns_filtered_projections() {
    let storage = InMemoryStorage::new();
    let store = ProjectionStore::new(storage.clone());

    // Save some test projections
    let student1 = StudentShortInfo { /* premium */ };
    let student2 = StudentShortInfo { /* basic */ };
    let student3 = StudentShortInfo { /* premium */ };

    store.save_async("s1", &student1).await.unwrap();
    store.save_async("s2", &student2).await.unwrap();
    store.save_async("s3", &student3).await.unwrap();

    // Query by tag
    let premium_students = store
        .query_by_tag_async(Tag {
            key: "EnrollmentTier".to_string(),
            value: "Premium".to_string(),
        })
        .await
        .unwrap();

    assert_eq!(premium_students.len(), 2);
}
```

## Troubleshooting

### Indices Not Being Created

**Symptom:** Queries return empty results even though projections exist.

**Causes:**
1. Tag provider not registered
2. ProjectionStore not using the tag provider
3. Projection not rebuilt after adding tags

**Solution:** Delete projection folder and restart to force rebuild with new tag provider.

### Stale Indices

**Symptom:** Query results don't match actual projection state.

**Causes:**
1. Manual file edits bypassing the store
2. Incomplete rebuild due to crash

**Solution:** 
```rust
// Force rebuild projection indices
// (Delete index folder and resave all projections)
```

### Performance Issues

**Symptom:** Tag queries slower than expected.

**Causes:**
1. Too many tags per projection (index overhead)
2. High-cardinality tag values (many unique index files)
3. Queries returning large result sets still need memory

**Solution:** 
- Review tag strategy—use only truly filterable properties
- Consider reducing cardinality (fewer distinct values)
- Paginate large result sets if needed

## Related Documentation

- [Projections](./projections.md) — Core projection concepts
- [Ledger Management](./ledger-management.md) — Sequence position tracking
- [Event Indexing](./event-indexing.md) — How events are indexed
