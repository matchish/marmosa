# Ubuntu Test Failures Analysis

Linux CI failures often reveal path, permission, and timing assumptions hidden on other platforms.

## Common causes

- Case sensitivity differences.
- Permission and ownership mismatches.
- Assumed directory existence.
- Timing-sensitive async tests.

## Mitigation

- Normalize paths and directory creation behavior.
- Strengthen async test determinism.
- Prefer explicit retries/timeouts only where justified.
