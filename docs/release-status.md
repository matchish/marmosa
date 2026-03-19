# Release Readiness Status

Status is release-ready when all items below are green.

## Required Checks

- Build passes in release mode.
- All tests pass.
- Doctests pass.
- `Cargo.toml` metadata is complete and version is correct.
- Changelog is updated for the target version.

## Minimal Commands

```bash
cargo build --release
cargo test
cargo test --doc
cargo package
```

## Go/No-Go

Proceed with publish only when every command above succeeds without unresolved blockers.
