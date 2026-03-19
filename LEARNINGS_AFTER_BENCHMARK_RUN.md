# Learnings After Benchmark Run

## Benchmark Setup

Marmosa was benchmarked against umadb using the `event-store-benchmark` suite.

- **Workload:** `concurrent_readers` — prepopulates 10,000 events across 1,000 streams, then N readers each read `limit=100` events from random streams for 6 seconds.
- **Adapter:** A `TokioFsBackend` implementing `StorageBackend` with `tokio::fs` was used.

## Results Summary

| Metric | marmosa (1 reader) | umadb (1 reader) | Ratio |
|---|---|---|---|
| Throughput (events/sec) | ~52 | ~69,765 | **~1,340x slower** |
| p50 latency | 256 ms | 0.19 ms | **~1,350x slower** |

Read performance degrades further under concurrency. Write performance is reasonable (~33k events/sec).

## Root Cause: Full Table Scan on Every Read

`read_internal()` in `src/event_store/mod.rs` performs a **full scan of all event files** for every read, regardless of the query:

```
for current_pos in filtered_positions {          // iterates ALL positions
    let data = self.storage.read_file(&file_path).await?;  // opens each file
    let record = serde_json::from_slice(&data)?;            // deserializes each event
    if query.matches(&record) {                             // filters AFTER reading
        results.push(record);
    }
}
```

To return ~10 events for `stream-42`, marmosa opens and deserializes **all 10,000 event files**, then discards 9,990 of them.

### What Opossum (C#) Does Differently

The C# reference implementation resolves queries through **indices before touching event files**:

1. Reads `Indices/Tags/stream_stream-42.json` → gets `[42, 1042, 2042, ...]` (only ~10 positions)
2. Opens **only those ~10 event files**
3. Uses `Parallel.ForEachAsync` for batches >10 files

This turns an O(N) full scan into an O(index_read + matched_count) lookup.

### Marmosa Has Indices — But Doesn't Use Them for Reads

`TagIndex` and `IndexManager` exist in `src/event_store/indices.rs` with:
- `add_position_async()` — adds event positions to tag-based index files
- `get_positions_by_tag_async()` — retrieves positions matching a tag

But `read_internal()` never calls them. It was ported as a simplified version that hasn't been optimized yet.

## Detailed Bottleneck Breakdown

Per single read of `stream-42` with 10,000 total events:

| Step | What happens | Cost |
|---|---|---|
| `read_dir("Events")` | Lists **all 10,000 filenames** | 1 syscall, 10K entries |
| Parse & sort | Parses 10K filenames → u64, sorts them | CPU, allocations |
| **10,000 × `read_file`** | Opens each file, reads bytes | **10,000 open+read+close syscalls** |
| **10,000 × JSON deserialize** | `serde_json::from_slice` per event | CPU per event |
| `query.matches()` | Checks tag match — only ~10 of 10,000 match | Wasted work on 9,990 events |

## Secondary Issues

1. **One file per event** — even with index-based narrowing, 10K individual files means high syscall overhead for bulk operations.
2. **No caching** — directory listings and parsed filenames aren't cached; every read re-enumerates the Events directory.
3. **Global write lock** — `append_async` acquires `acquire_stream_lock("global_store")`, serializing all writes. Not a reader issue, but affects mixed workloads.
4. **`Mutex<BTreeSet>` for locks** — the `TokioFsBackend` adapter uses a std `Mutex` inside an async context, which can block the tokio runtime under contention.

## Recommended Fixes (Priority Order)

1. **Wire up `IndexManager` in `read_internal()`** — resolve query items through tag/event-type indices first, then only read matched positions. This alone should close most of the 1,000x gap.
2. **Ensure indices are populated during `append_async()`** — verify that `IndexManager::add_event_to_indices_async()` is called when events are written.
3. **Parallel file reads** for matched positions (like C# does with `Parallel.ForEachAsync`).
4. **Cache directory listings** or maintain an in-memory position set to avoid `read_dir` on every read.
