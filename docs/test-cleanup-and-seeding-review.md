# Test Cleanup and Seeding Review

Reliable tests require explicit setup and teardown.

## Guidance

- Seed only required test data.
- Isolate test data per test/module.
- Clean temporary paths on completion.
- Avoid hidden coupling between tests.
