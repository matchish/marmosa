# Analysis: `Metadata/index.json` Purpose and Alternatives

## What is `index.json`?

The `Metadata/index.json` file is a **centralized metadata index** that stores projection metadata for ALL projection keys in a single JSON file.

**Location:** `Projections/{ProjectionName}/Metadata/index.json`

**Structure:**
```json
{
  "user-123": {
    "version": 5,
    "created_at": 1234567890,
    "last_updated_at": 1234567999,
    "size_in_bytes": 512
  },
  "user-456": {
    "version": 2,
    "created_at": 1234567800,
    "last_updated_at": 1234567850,
    "size_in_bytes": 256
  }
}
```

**Metadata fields:**
- `version` - Incremented on each update (optimistic concurrency)
- `created_at` - Timestamp when projection was first created
- `last_updated_at` - Timestamp of most recent update
- `size_in_bytes` - Size of the serialized projection data

## Purpose

The centralized index enables:

1. **Fast metadata queries** - Get metadata without reading individual projection files
2. **List all projections** - `GetAllAsync()` returns all keys with metadata
3. **Query by timestamp** - `GetUpdatedSinceAsync()` finds recently updated projections
4. **Version tracking** - Supports optimistic concurrency control

## The Problem

Every `save()` operation in Marmosa currently:

```rust
// 1. Read entire index.json
let metadata_index = storage.read_file(&metadata_index_path).await;

// 2. Deserialize full map
let mut metadata_index: BTreeMap<String, ProjectionMetadata> =
    serde_json::from_slice(&data);

// 3. Modify one entry
metadata_index.insert(key, current_metadata);

// 4. Serialize entire map
let index_data = serde_json::to_vec(&metadata_index);

// 5. Write it back
storage.write_file(&metadata_index_path, index_data).await;
```

**Complexity:** O(N) per save, where N = number of projection keys

**Impact:**
- With 10,000 projections, every save reads/writes ~1MB+ of JSON
- Serialization/deserialization overhead grows linearly
- Write amplification: 1 key update rewrites all keys

---

## Alternatives

### 1. Per-file Embedded Metadata

Store metadata inside each projection file using a wrapper structure.

**Implementation:**
```rust
struct ProjectionWrapper<T> {
    data: T,
    metadata: ProjectionMetadata,
}
```

**File:** `Projections/{Name}/{key}.json`
```json
{
  "data": { /* actual projection state */ },
  "metadata": {
    "version": 5,
    "created_at": 1234567890,
    "last_updated_at": 1234567999,
    "size_in_bytes": 512
  }
}
```

| Pros | Cons |
|------|------|
| O(1) per save - only write affected file | Must read file to get metadata |
| No index corruption risk | `GetAllAsync()` requires directory scan |
| Already implemented in Marmosa (`ProjectionWrapper`) | `GetUpdatedSinceAsync()` is expensive |

**Note:** Marmosa already embeds metadata in projection files via `ProjectionWrapper`. The index.json is redundant for individual lookups.

---

### 2. Lazy/Cached Index (Opossum's Approach)

Keep index in memory, persist to disk periodically or on changes.

**Implementation (from Opossum):**
```csharp
internal sealed class ProjectionMetadataIndex
{
    private readonly ConcurrentDictionary<string, ProjectionMetadata> _cache = new();
    private readonly SemaphoreSlim _indexLock = new(1, 1);

    public async Task SaveAsync(string projectionPath, string key, ProjectionMetadata metadata)
    {
        await _indexLock.WaitAsync();
        try
        {
            _cache[key] = metadata;           // Update in-memory
            await PersistIndexAsync(projectionPath);  // Write to disk
        }
        finally { _indexLock.Release(); }
    }
}
```

| Pros | Cons |
|------|------|
| Fast in-memory lookups | Still O(N) disk writes |
| Cache reduces read I/O | Memory usage grows with projections |
| Thread-safe with locks | Requires proper lifecycle management |

---

### 3. Skip Index During Rebuild

Don't write `index.json` during rebuilds. Rebuild it lazily on first read.

**From Opossum CHANGELOG:**
> "Post-rebuild reads are served from per-file embedded metadata, eliminating the O(unique_keys) JSON blob that was written to `Metadata/index.json` at commit time."

**Implementation:**
- During rebuild: only write projection files (with embedded metadata)
- After rebuild: `index.json` doesn't exist
- On first metadata query: scan projection files, build index, cache it

| Pros | Cons |
|------|------|
| Rebuilds are much faster | First query after rebuild is slow |
| Reduces write amplification | Requires graceful handling of missing index |
| Simpler rebuild logic | |

---

### 4. Key-Value Store (SQLite, LMDB, sled)

Replace JSON file with an embedded database.

**Example with sled (Rust):**
```rust
let db = sled::open("projections.db")?;
let metadata_tree = db.open_tree("metadata")?;

// O(log N) insert
metadata_tree.insert(key.as_bytes(), serde_json::to_vec(&metadata)?)?;

// O(log N) lookup
let data = metadata_tree.get(key.as_bytes())?;
```

| Pros | Cons |
|------|------|
| O(log N) updates | Adds dependency |
| ACID transactions | Not no_std compatible |
| Built-in caching | More complex deployment |
| Crash-safe | Different file format |

**Candidates:**
- `sled` - Pure Rust, embedded
- `redb` - Pure Rust, simpler than sled
- `SQLite` - Battle-tested, but C dependency
- `LMDB` - Very fast, but C dependency

---

### 5. Directory-Based Metadata

Store each key's metadata as a separate file.

**Structure:**
```
Projections/{Name}/Metadata/
  user-123.json  -> {"version": 5, "created_at": ..., ...}
  user-456.json  -> {"version": 2, "created_at": ..., ...}
```

| Pros | Cons |
|------|------|
| O(1) updates | Many small files (filesystem overhead) |
| No serialization of full index | `GetAllAsync()` requires directory listing |
| Simple implementation | May hit inode limits on some filesystems |
| Atomic per-key updates | |

---

### 6. Append-Only Log with Compaction

Write metadata changes as log entries, compact periodically.

**Log file:** `Projections/{Name}/Metadata/changes.log`
```
{"op":"set","key":"user-123","metadata":{...},"ts":1234567890}
{"op":"set","key":"user-456","metadata":{...},"ts":1234567891}
{"op":"delete","key":"user-123","ts":1234567900}
```

**Compacted index:** `Projections/{Name}/Metadata/index.json` (rebuilt periodically)

| Pros | Cons |
|------|------|
| O(1) append writes | Requires compaction process |
| Full history available | Log grows unbounded without compaction |
| Crash recovery friendly | Read requires log replay |
| Good for event-sourced systems | More complex implementation |

---

## Opossum's Evolution

Opossum uses a **hybrid approach**:

1. **Embedded metadata** in each projection file (`ProjectionWrapper`)
2. **In-memory cache** with `ConcurrentDictionary`
3. **Lazy centralized index** that can be rebuilt from embedded metadata
4. **Skip index writes during rebuild** for performance

Key insight from Opossum's `ProjectionMetadataIndex`:
```csharp
/// Clears the in-memory metadata cache without touching disk.
/// Called after a projection rebuild to discard stale entries, because the
/// aggregated Metadata/index.json no longer exists in the swapped-in directory.
/// The next GetAsync call will fall through to LoadIndexAsync,
/// which handles a missing index file gracefully.
internal void ClearCache() => _cache.Clear();
```

---

## Recommendations for Marmosa

### Short-term (Quick Win)
**Skip `index.json` writes during rebuilds**
- Marmosa already embeds metadata in `ProjectionWrapper`
- Handle missing `index.json` gracefully in queries
- Build index lazily on first metadata query post-rebuild

### Medium-term (Better Performance)
**Make embedded metadata the primary source**
- Remove `index.json` as source of truth
- Use in-memory cache for fast lookups
- Rebuild cache from directory scan when needed

### Long-term (Embedded/ESP32 Targets)
**Consider append-only log**
- Aligns with event-sourcing philosophy
- Minimal write amplification
- Works well with flash storage (wear leveling)
- Can be compacted during idle periods

---

## Implementation Priority

| Change | Effort | Impact | Priority |
|--------|--------|--------|----------|
| Skip index.json during rebuild | Low | High | 1 |
| Handle missing index.json gracefully | Low | Medium | 2 |
| Add in-memory cache layer | Medium | High | 3 |
| Remove index.json dependency entirely | Medium | Medium | 4 |
| Evaluate sled/redb for std targets | High | Low | 5 |

---

## File References

**Marmosa:**
- Index usage: `src/projections.rs` (lines 510-551, 587-599)
- ProjectionWrapper: `src/projections.rs` (lines 534-537)

**Opossum:**
- MetadataIndex: `src/Opossum/Projections/ProjectionMetadataIndex.cs`
- CHANGELOG notes: `CHANGELOG.md` (lines 115-119)
