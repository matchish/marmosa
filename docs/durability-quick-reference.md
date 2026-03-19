# Durability Quick Reference (Marmosa)

## TL;DR

- Production: prioritize durable writes for events and sequence bookkeeping.
- Tests/dev: reduced durability can speed test feedback where data loss is acceptable.

## Durability Priorities

| Data class | Durability priority | Why |
|---|---|---|
| Events | High | source of truth |
| Ledger / sequence state | High | ordering and append safety |
| Projections | Medium | rebuildable from events |
| Derived indices | Medium | rebuildable from events |

## Operational Guidance

- Keep event append path crash-safe before optimizing throughput.
- Benchmark with and without stricter fsync-like behavior on target storage.
- Use SSD/NVMe for append-heavy workloads.
- Treat projection/index rebuild as recovery path, not primary consistency boundary.

## Decision Tree

```text
Need strongest crash safety?
  yes -> durable writes first, then optimize
  no  -> relax durability only in test/dev
```
