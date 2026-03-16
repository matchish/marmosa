# Cloudflare R2/KV Backend

## Status: Not Started

## Description
Implement `StorageBackend` for Cloudflare Workers using R2 (object storage) or KV (key-value store).

## Acceptance Criteria
- [ ] `CloudflareR2Backend` implementing `StorageBackend`
- [ ] Uses `worker` crate bindings
- [ ] Proper error mapping to our `Error` enum
- [ ] Lock mechanism using KV with TTL
- [ ] Gated behind `#[cfg(target_arch = "wasm32")]` or feature flag

## API Design
```rust
#[cfg(feature = "cloudflare")]
pub struct CloudflareR2Backend {
    bucket: worker::R2Bucket,
    kv: worker::KvStore,  // For locks
}

impl StorageBackend for CloudflareR2Backend {
    // R2 for file storage
    // KV for distributed locks with TTL
}
```

## Considerations
- R2 has eventual consistency - may need read-after-write handling
- KV has 60-second TTL minimum - affects lock timeout
- Billing considerations for high-frequency operations

## Dependencies
- StorageBackend trait (done)
- `worker` crate
