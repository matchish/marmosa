# Testing Configuration Override Fix

Tests should explicitly override runtime configuration to prevent environmental leakage.

## Guidance

- Set storage roots to temporary test paths.
- Override feature flags explicitly in tests.
- Keep defaults stable for production code.
