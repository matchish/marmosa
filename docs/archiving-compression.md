# Projection Archiving & Compression (Design Guide)

## Overview

Archiving moves inactive projection data from primary storage into compressed archive files while keeping enough metadata to audit and optionally restore data.

Target outcomes:

- reduce active storage pressure
- preserve long-term records
- avoid accidental projection rebuild loops after archival

## Current Status

This document is a migrated design from the C# source specifications.

The crate currently has no finalized public archiving API (no archive/restore module in `src`). Use this as implementation guidance.

## Suggested Rust Model

Prefer explicit, composable Rust types:

```rust,ignore
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    Zip,
}

#[derive(Debug, Clone)]
pub struct ArchiveConfig {
    pub archive_root: String,
    pub include_metadata: bool,
    pub compression_level: u8,
}
```

And service traits for behavior:

```rust,ignore
pub trait ProjectionArchiver {
    async fn archive(
        &self,
        projection_root: &str,
        key: &str,
        metadata: &ProjectionMetadata,
        config: &ArchiveConfig,
    ) -> Result<String, ArchiveError>;

    async fn restore(
        &self,
        archive_path: &str,
        projection_root: &str,
        key: &str,
    ) -> Result<(), ArchiveError>;
}
```

## Archive Layout

Recommended path convention:

`{archive_root}/{projection_name}/{year}/{month}/{key}.zip`

Recommended archive entries:

- `projection.json`
- `metadata.json` (optional)

## Retention Integration

Archiving should plug into retention decisions:

1. retention policy marks key as eligible
2. archiver writes compressed artifact
3. active projection file is deleted
4. tombstone-like marker records archive location

If archive creation fails, do not delete active projection data.

## Existing Building Blocks in This Crate

Current APIs useful for future implementation:

- `ProjectionMetadata` (timestamp/version/size)
- `ProjectionStore::delete` and projection file lifecycle behavior
- `ProjectionMetadataIndex` for candidate selection by age

## Safety Rules

- Archive operation should be all-or-nothing per key.
- Never lose data on partial failure.
- Keep audit metadata for every archived key.
- Log archive and restore actions with key and resulting path.

## Restore Guidance

Restore should:

- verify archive exists and contains expected entries
- write projection data back to active projection path
- restore/update metadata index entry
- clear tombstone marker

`Result`-based error handling is preferred; avoid panics during archive/restore flows.
