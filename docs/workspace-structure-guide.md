# Understanding Workspace Structure in Marmosa

This project uses Rust workspace conventions rather than `.slnx` solution manifests.

## Core Files

- `Cargo.toml`: package/workspace metadata and dependency definitions.
- `src/`: library/runtime code.
- `tests/`: integration tests via public API.
- `benches/`: benchmark targets.
- `docs/`: project documentation.

## Keeping Structure Healthy

- Keep docs in `/docs` only (no scattered markdown files).
- Keep unit tests near implementation (`#[cfg(test)]` blocks).
- Keep integration tests in `/tests`.
- Split large modules into focused files proactively.

## When Adding New Docs

1. Place file under `/docs`.
2. Use a clear, stable name.
3. Update cross-links from related docs where needed.

## Quick Validation

```bash
cargo check
cargo test --doc
```
