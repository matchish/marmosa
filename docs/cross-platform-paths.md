# Cross-Platform Paths

Marmosa paths should remain portable across macOS/Linux/Windows.

## Rules

- Use platform-safe path joins in application code.
- Avoid hardcoded drive letters or root assumptions.
- Keep test paths isolated and temporary.

## Porting lesson

Most path bugs come from implicit assumptions about separators and root prefixes.
