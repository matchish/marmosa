# Storage Path Configuration

Although Marmosa is file-system based, path configuration acts like database location strategy.

## Guidance

- Choose one durable root per environment.
- Keep event, projection, and index subpaths predictable.
- Do not reuse production paths in tests.
