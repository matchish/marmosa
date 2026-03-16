# Marmosa Backlog

## Overview
This directory contains the feature backlog for Marmosa, a Rust port of [Opossum](https://github.com/MajorMartinDev/Opossum) - a file-based event store with DCB (Dynamic Consistency Boundaries) support.

## Completed Features ✅
- [x] Core domain types (Tag, Query, DomainEvent, EventRecord, AppendCondition)
- [x] StorageBackend trait with InMemoryStorage test double
- [x] EventStore trait with OpossumStore implementation
- [x] DCB-compliant append conditions (fail_if_events_match + after_sequence_position)
- [x] ProjectionDefinition trait
- [x] ProjectionStore trait (pluggable)
- [x] StorageBackendProjectionStore (default file-based)
- [x] ProjectionRunner with checkpoint support

## Backlog (Prioritized)

### Phase 1: Core Functionality
| # | Feature | Priority | Complexity |
|---|---------|----------|------------|
| 001 | [Integration Test: EventStore + Projections](001-integration-test-eventstore-projections.md) | High | Low |
| 002 | [Tag-Based Projection Indices](002-tag-based-projection-indices.md) | High | Medium |
| 003 | [Projection Rebuild](003-projection-rebuild.md) | High | Medium |
| 004 | [Event Indexing Engine](004-event-indexing.md) | High | High |
| 013 | [Cross-Process Safety & Locking](013-cross-process-locks.md) | High | High |
| 014 | [Event Store Crash Recovery](014-crash-recovery.md) | High | High |
| 015 | [Advanced Event Reading Queries](015-advanced-event-reading.md) | High | Medium |

### Phase 2: Storage Backends
| # | Feature | Priority | Complexity |
|---|---------|----------|------------|
| 005 | [Filesystem Backend (std)](005-filesystem-backend-std.md) | High | Medium |
| 006 | [Cloudflare R2/KV Backend](006-cloudflare-r2-backend.md) | Medium | Medium |
| 007 | [ESP32 Flash Backend](007-esp32-flash-backend.md) | Medium | High |

### Phase 3: Projection Backends
| # | Feature | Priority | Complexity |
|---|---------|----------|------------|
| 008 | [PostgreSQL Projection Store](008-postgres-projection-store.md) | Medium | Medium |

### Phase 4: Advanced Features
| # | Feature | Priority | Complexity |
|---|---------|----------|------------|
| 009 | [Mediator Pattern](009-mediator-pattern.md) | Medium | Medium |
| 010 | [Snapshot Support](010-snapshot-support.md) | Low | High |
| 011 | [Retention Policies](011-retention-policies.md) | Low | Medium |
| 012 | [Live Subscriptions](012-subscription-live-updates.md) | Low | High |
| 016 | [Event Store Admin & Maintenance](016-admin-maintenance-api.md) | Low | Medium |
| 017 | [CQRS Decision Models](017-decision-models.md) | Medium | High |
| 018 | [Parallel Projection Rebuilding](018-parallel-projection-rebuild.md) | Low | High |

## Design Principles
1. **`no_std` first** - Core functionality works without std library
2. **Pluggable backends** - Storage and projection stores are trait-based
3. **TDD** - Every feature starts with failing tests
4. **Opossum compatibility** - Follow DCB specification and Opossum patterns

## Contributing
1. Pick a feature from the backlog
2. Write failing tests first
3. Implement until tests pass
4. Update this README with completion status
