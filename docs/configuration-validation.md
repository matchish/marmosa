# Configuration Validation

Configuration should fail fast when invalid rather than producing partial runtime behavior.

## Validation checklist

- Required paths are present and writable.
- Context/store names are valid and stable.
- Projection configuration is internally consistent.
- Feature flags/defaults are explicit.

## Recommendation

Validate once at startup and return actionable errors.
