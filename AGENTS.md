The project is port of Opossum C# library
When porting prefer follow Rust idiomatic ways
THe project is under heavy development so breaking/significant changes acceptable

Do not commit .py files and other helper files

### Testing Conventions
- **Unit Tests:** Always place unit tests at the bottom of the same file they are testing, wrapped in a `#[cfg(test)] mod tests { ... }` block. Do **not** create separate `_tests.rs` files inside the `src/` directory.
- **Integration Tests:** Place tests that only use the public API in the `tests/` directory at the root of the project.

### Refactoring Guidelines
- **File Splitting:** Do not proactively refactor or split code into separate files just because a file seems "too big". Keep code in its original destination or existing structures unless explicitly asked by the user to split it out. Maintain the 1:1 conceptual mapping during the porting process rather than aggressively modularizing.
