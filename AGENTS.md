The project is port of Opossum C# library
When porting prefer follow Rust idiomatic ways
THe project is under heavy development so breaking/significant changes acceptable

Do not commit .py files
### Testing Conventions
- **Unit Tests:** Always place unit tests at the bottom of the same file they are testing, wrapped in a `#[cfg(test)] mod tests { ... }` block. Do **not** create separate `_tests.rs` files inside the `src/` directory.
- **Integration Tests:** Place tests that only use the public API in the `tests/` directory at the root of the project.
