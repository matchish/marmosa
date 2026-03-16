# Native Filesystem Backend (std feature)

## Status: Not Started

## Description
Implement a real filesystem backend using `tokio::fs`, gated behind `#[cfg(feature = "std")]` to preserve `no_std` compatibility.

## Acceptance Criteria
- [ ] `FileSystemBackend` struct implementing `StorageBackend`
- [ ] Uses `tokio::fs` for async file operations
- [ ] Directory creation with proper error handling
- [ ] File read/write with atomic writes (write to temp, rename)
- [ ] Cross-platform path handling (Windows/Linux/macOS)
- [ ] Integration tests with real temp directories

## API Design
```rust
#[cfg(feature = "std")]
pub struct FileSystemBackend {
    root_path: PathBuf,
}

#[cfg(feature = "std")]
impl FileSystemBackend {
    pub fn new(root_path: impl AsRef<Path>) -> Self;
}
```

## Cargo.toml
```toml
[features]
default = []
std = ["tokio/fs", "tokio/sync"]
```

## Dependencies
- StorageBackend trait (done)
