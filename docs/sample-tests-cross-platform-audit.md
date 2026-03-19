# Sample Tests Cross-Platform Audit

Sample and integration tests should avoid platform-specific assumptions.

## Checklist

- Temporary directories for all filesystem tests.
- No hardcoded absolute paths.
- Stable line endings and path handling.
- Cleanup of created files/directories.
