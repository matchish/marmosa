# Parallel Projection Rebuild: Implementation Summary

Porting work explored strategies for parallel projection rebuild while preserving correctness.

## Key points

- Maintain deterministic ordering where required by projection semantics.
- Isolate per-projection state to reduce contention.
- Keep checkpoint behavior explicit to support restart safety.

## Current stance

Parallel rebuild should be enabled only when projection dependencies and ordering requirements are
well understood.
