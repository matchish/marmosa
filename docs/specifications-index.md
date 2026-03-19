# Marmosa Specifications Index

This document tracks high-level feature specifications migrated from `docs_opossum/specifications`
into Rust-oriented documentation.

## Current Spec Set

| Spec | Rust doc | Scope | Status | Priority |
|---|---|---|---|---|
| SPEC-001 | Inline docs in `src/projections.rs` | Projection metadata and metadata index behavior | Migrated | P0 |
| SPEC-002 | `docs/cache-warming.md` | Cache warming design | Migrated | P1 |
| SPEC-003 | `docs/retention-policies.md` | Retention lifecycle design | Migrated | P1 |
| SPEC-004 | `docs/archiving-compression.md` | Archive/compress design | Migrated | P2 |
| SPEC-005 | `docs/smart-retention.md` | Access-aware retention design | Migrated | P3 (Future) |

## DCB High-Level Specifications

| Source concept | Rust doc | Status |
|---|---|---|
| DCB Specification | `docs/dcb-specification.md` | Migrated |
| DCB Projections | `docs/dcb-projections.md` | Migrated |

## Dependency Order (Design-Level)

```text
SPEC-001 (Projection Metadata)
  ├─ SPEC-002 (Cache Warming)
  ├─ SPEC-003 (Retention Policies)
  │   ├─ SPEC-004 (Archiving & Compression)
  │   └─ SPEC-005 (Smart Retention)
  ├─ SPEC-004 (Archiving & Compression)
  └─ SPEC-005 (Smart Retention)
```

## Design Principles Used in Migration

- Behavior-first translation from C# naming to Rust crate terminology.
- Item docs near code (`///`/`//!`) when concrete APIs exist.
- Concept docs in `/docs` when design spans multiple modules or is future-facing.
- Doctest-safe snippets (`rust`, `rust,no_run`, or `rust,ignore` as appropriate).

## Notes

- This index tracks documentation migration status, not full implementation status.
- Some docs (for example cache/retention/archive/smart-retention) are currently design guides
  and may precede runtime implementation.
