The project is port of Opossum C# library
When porting prefer follow Rust idiomatic ways
THe project is under heavy development so breaking/significant changes acceptable

Do not commit .py files and other helper files

### Testing Conventions
- **Unit Tests:** Always place unit tests at the bottom of the same file they are testing, wrapped in a `#[cfg(test)] mod tests { ... }` block. Do **not** create separate `_tests.rs` files inside the `src/` directory.
- **Integration Tests:** Place tests that only use the public API in the `tests/` directory at the root of the project.

### Refactoring Guidelines
- **File Splitting:** Proactively prevent files from becoming too big. Break down large files into smaller, well-organized modules and separate files. Do not keep all ported code in a single massive file just because it was that way in the original C# codebase (or for any other reason). Use Rust's module system properly to keep individual source files focused and manageable in size.
