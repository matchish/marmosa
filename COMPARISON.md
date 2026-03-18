# Opossum vs Marmosa: Performance Comparison

This document compares the performance characteristics of the original Opossum (.NET/C#) event store with its Rust port, Marmosa.

## Executive Summary

Opossum has index-based optimizations that Marmosa is currently missing. Before considering advanced optimizations like bloom filters, Marmosa should port Opossum's existing indexing infrastructure.

## Detailed Comparison

### 1. Event Append Condition Checking

| Aspect | Opossum | Marmosa |
|--------|---------|---------|
| **Approach** | Index-based lookup | Full event scan |
| **Complexity** | O(index size) | O(N) where N = total events |
| **Implementation** | Uses `IndexManager` via `GetPositionsForQueryAsync()` | Reads all events via `read_internal(Query::all(), ...)` |

**Opossum Implementation:**
- Location: `src/Opossum/Storage/FileSystem/FileSystemEventStore.cs` (lines 413-464)
- `ValidateAppendConditionAsync()` leverages the same index infrastructure used for queries
- When `AfterSequencePosition` is set, filters results to only check positions after that point
- Falls back to simple ledger position comparison when no specific query items exist

**Marmosa Implementation:**
- Location: `src/event_store/mod.rs` (lines 139-148)
- Calls `read_internal(Query::all(), after_sequence_position, None, None)`
- Iterates through ALL events checking `cond.fail_if_events_match.matches(&evt)`
- Reads and deserializes every event file from disk

**Gap:** Marmosa scans all events while Opossum uses indexed lookups.

---

### 2. Event Query Optimization

| Aspect | Opossum | Marmosa |
|--------|---------|---------|
| **Approach** | Index-driven query resolution | Full sequential scan |
| **Complexity** | O(index size) | O(N) where N = total events |
| **Indexing** | Separate indices for EventTypes and Tags | No event-level indexing |

**Opossum Implementation:**
- Location: `src/Opossum/Storage/FileSystem/FileSystemEventStore.cs` (lines 278-373)
- Maintains separate indices stored in JSON files
- `GetPositionsForQueryAsync()` uses `IndexManager` for position lookups
- Parallelizes multi-type queries with `Task.WhenAll()`
- Intersects tag position sets using HashSet logic
- Uses k-way `SortedMerge()` for efficient deduplication (lines 189-229)

**Marmosa Implementation:**
- Location: `src/event_store/mod.rs` (lines 54-109)
- Lists all files in `Events/` directory
- Sorts the full position list
- For each position, reads entire event file and checks `query.matches(&record)`

**Gap:** Marmosa has no index acceleration; every query reads all events.

---

### 3. Projection Key Existence Checks

| Aspect | Opossum | Marmosa |
|--------|---------|---------|
| **Approach** | File metadata check | Full file read |
| **Complexity** | O(1) | O(file size) |

**Opossum Implementation:**
- Location: `src/Opossum/Projections/FileSystemProjectionStore.cs` (lines 60-101)
- Uses `File.Exists(filePath)` for O(1) metadata check
- Only reads file content if it exists

**Marmosa Implementation:**
- Location: `src/projections.rs` (lines 444-459)
- Always attempts `read_file(&path)` then deserializes
- Returns `Ok(None)` from error handler if file doesn't exist

**Gap:** Marmosa pays full read cost even for non-existent keys.

---

### 4. Stream Deduplication

| Aspect | Opossum | Marmosa |
|--------|---------|---------|
| **Approach** | k-way merge algorithm | Position-based filtering |
| **Complexity** | O(N × K) for index merging | O(1) for checkpoint comparison |

**Opossum Implementation:**
- Location: `src/Opossum/Storage/FileSystem/IndexManager.cs` (lines 189-229)
- k-way merge merges pre-sorted arrays without duplicates
- Uses position tracking arrays with minimal O(K) memory overhead
- Checkpoint-based filtering in `ProjectionManager.cs`

**Marmosa Implementation:**
- Location: `src/projections.rs` (lines 201-269)
- Simple checkpoint position comparison
- Uses BTreeSet intersection for multi-tag queries

**Assessment:** Both handle checkpoint-based deduplication effectively. Opossum's k-way merge is more optimized for index operations.

---

## Summary Table

| Area | Opossum | Marmosa | Severity |
|------|---------|---------|----------|
| Append Condition Checking | Index-based O(index) | Full scan O(N) | **HIGH** |
| Event Query Performance | Indexed O(index) | Full scan O(N) | **CRITICAL** |
| Key Existence Checks | File metadata O(1) | Full read O(size) | MEDIUM |
| Stream Deduplication | k-way merge O(N×K) | Position filter O(1) | LOW |

---

## Opossum Optimizations Missing in Marmosa

1. **Event Type Index** - `EventTypeIndex.cs` maintains positions by event type
2. **Tag Index** - `TagIndex.cs` maintains positions by tag key/value
3. **Parallel index reads** - `Task.WhenAll()` for multi-type/tag queries
4. **k-way merge deduplication** - More efficient than HashSet for sorted data
5. **Checkpoint caching** - In-memory `_checkpointCache` eliminates per-tick file reads
6. **Per-projection locks** - Prevents concurrent rebuild/update conflicts
7. **Atomic rebuild** - Write-through temp directory with atomic swap
8. **Tag accumulator flushing** - Batches tag index updates during rebuild

---

## Bloom Filter Analysis

### Impact by Area

| Area | Bloom Filter Value for Opossum | Bloom Filter Value for Marmosa |
|------|-------------------------------|--------------------------------|
| Append Condition Checking | Low (already indexed) | High (currently O(N)) |
| Event Query Optimization | Low (already indexed) | High (currently O(N)) |
| Projection Key Existence | Low (O(1) metadata check) | Medium (avoids failed reads) |
| Stream Deduplication | Low | Low |

### Recommendation

**Marmosa should first port Opossum's indexing strategy before considering bloom filters.**

Bloom filters are a micro-optimization; the macro-optimization (indexing) is what Opossum already implements and Marmosa currently lacks.

### Where Bloom Filters Could Still Help Opossum

- **Index file reads** - Before loading a tag index file from disk, a bloom filter could confirm whether that tag has any events, avoiding unnecessary I/O for rare/nonexistent tags
- **Very high cardinality scenarios** - With millions of unique tags, bloom filters could reduce index lookup overhead

---

## File Reference

### Opossum
- Event store: `src/Opossum/Storage/FileSystem/FileSystemEventStore.cs`
- Indices: `src/Opossum/Storage/FileSystem/IndexManager.cs`, `EventTypeIndex.cs`, `TagIndex.cs`
- Projections: `src/Opossum/Projections/FileSystemProjectionStore.cs`, `ProjectionManager.cs`

### Marmosa
- Event store: `src/event_store/mod.rs`
- Indices: `src/event_store/indices.rs` (projection-level only)
- Projections: `src/projections.rs`
- Domain: `src/domain.rs`
