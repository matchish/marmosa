# ESP32 Flash Storage Backend

## Status: Not Started

## Description
Implement `StorageBackend` for ESP32 embedded devices using flash storage (SPIFFS/LittleFS).

## Acceptance Criteria
- [ ] `Esp32FlashBackend` implementing `StorageBackend`
- [ ] Uses `embedded-storage` traits
- [ ] Wear leveling considerations
- [ ] Size limits appropriate for embedded (configurable max event size)
- [ ] No heap allocation in hot paths where possible

## API Design
```rust
#[cfg(feature = "esp32")]
pub struct Esp32FlashBackend<F: Flash> {
    flash: F,
    partition_offset: u32,
    partition_size: u32,
}
```

## Considerations
- Flash write cycles are limited (~10k-100k)
- Power loss during write = corruption risk
- May need simpler file format (not JSON)
- Consider binary serialization (postcard crate)

## Dependencies
- StorageBackend trait (done)
- `embedded-storage` traits
- `esp-idf-hal` or `esp-hal`
