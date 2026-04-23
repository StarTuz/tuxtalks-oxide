# Contributing to TuxTalks Oxide (Rust)

## Behavioral source of truth

The **Python application at the repository root** (`tuxtalks.py`, `core/`, `players/`, etc.) is the **only** authority for correct *behavior*. Rust should match it.

- Use Python source and tests when implementing, reviewing, or debugging.
- Prefer parity over Rust-specific reinterpretations of product logic.
- Stack details (async, crates, D-Bus vs in-process APIs) may differ; **observable behavior** should not, unless maintainers agree and the exception is documented (e.g. in code comments or `CLAUDE.md`).
- **Config on disk is not shared with Python:** use `~/.config/tuxtalks-oxide/config.json` (same JSON *shape* as Python’s file, different path). Never read or write `~/.config/tuxtalks/config.json` from this crate.

## The "Top 1%" Engineering Standard

All contributions must adhere to the highest standards of software engineering.

### 1. Zero Warnings Policy

- Code must pass `cargo clippy --all-targets --all-features -- -D warnings -D clippy::pedantic`.
- No `unwrap()` or `expect()` in library code. Use proper error handling.
- No `todo!()` or `unimplemented!()` in merged code.

### 2. Testing Requirements

- Every new feature must have unit tests.
- Logic affecting search/matching must have property-based tests.
- Bug fixes must include a regression test.

### 3. Documentation

- All public items must have `///` doc comments.
- Complex logic must be explained in the `/docs` directory.

### 4. Style

- Use `cargo fmt` before committing.
- Follow the patterns established in `src/lib.rs`.

## Development Workflow

1. Run `make check` to ensure correctness.
2. Run `make lint` to check style and lints.
3. Run `make test` to verify logic.
4. Run `make audit` for security check.
5. (Recommended) Run `claude` for a secondary logic review and test generation.
