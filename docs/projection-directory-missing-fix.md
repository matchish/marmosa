# Projection Directory Missing Fix

Projection operations should not assume pre-existing directory structure.

## Guidance

- Ensure projection root/directories are created lazily before writes.
- Treat missing paths during reads as empty state where appropriate.
- Keep behavior consistent across event/projection/index subsystems.
