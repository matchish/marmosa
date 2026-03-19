# Linux Permission Fix Notes

Filesystem-backed systems rely on correct directory and file permissions.

## Guidance

- Ensure process user can create/read/write configured roots.
- Validate lock and checkpoint directories at startup.
- Surface permission errors with clear messages.
