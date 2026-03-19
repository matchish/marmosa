# Quick Release Commands (Marmosa)

Use this as a short checklist for publishing a new crate version.

## Pre-flight

```bash
# from repository root
cargo clean
cargo build --release
cargo test
cargo test --doc
```

## Release Steps

```bash
# 1) verify Cargo.toml version was bumped
grep '^version\s*=\s*"' Cargo.toml

# 2) package validation
cargo package

# 3) create git tag (example)
git tag -a v0.1.0 -m "marmosa v0.1.0"

# 4) publish crate
cargo publish

# 5) push tag after publish succeeded
git push origin v0.1.0
```

## Verify

```bash
# package page (after indexing)
open https://crates.io/crates/marmosa

# docs page (if docs.rs build succeeded)
open https://docs.rs/marmosa
```

## Troubleshooting

| Issue | Typical cause | Action |
|---|---|---|
| `already uploaded` | same version exists | bump `Cargo.toml` version |
| publish timeout | network/indexing issues | retry `cargo publish` once |
| docs.rs failed | docs build error/features | fix docs build and republish next version |
