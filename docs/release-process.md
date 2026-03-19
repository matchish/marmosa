# Release Process (crates.io)

This is the canonical release flow for publishing Marmosa.

## 1. Verify repository state

```bash
cargo clean
cargo build --release
cargo test
cargo test --doc
```

## 2. Verify package metadata

Check `Cargo.toml`:

- `name`, `version`, `description`, `license`, `repository`
- correct `readme` and categories/keywords if used

## 3. Validate package locally

```bash
cargo package
```

Optional: inspect archive in `target/package/`.

## 4. Tag release

```bash
git tag -a vX.Y.Z -m "marmosa vX.Y.Z"
```

## 5. Publish

```bash
cargo login <CRATES_IO_TOKEN>
cargo publish
```

## 6. Push tag and create GitHub release

```bash
git push origin vX.Y.Z
```

Create a matching GitHub release using changelog notes.

## 7. Post-release verification

- crates.io page updates for new version
- docs.rs build succeeds
- installation in a fresh project works

```bash
cargo new marmosa-smoke --bin
cd marmosa-smoke
echo 'marmosa = "X.Y.Z"' >> Cargo.toml
cargo check
```
