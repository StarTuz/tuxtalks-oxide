# Contributing to TuxTalks Oxide (Rust)

## Behavioral reference (Python, separate repo)

This repository is **Rust only**. The original Python app lives in a **different** project: [`StarTuz/tuxtalks`](https://github.com/StarTuz/tuxtalks) (local path is often `~/Code/tuxtalks/`). It is **not** the authority for lockstep parity; use it as a **behavioral reference** when porting a feature so we do not rediscover working semantics (MCWS shapes, normalizer rules, CLI UX).

- Consult Python source when implementing, reviewing, or debugging a ported slice.
- Stack details (async, crates, D-Bus vs in-process APIs) may differ; **observable behavior** for that slice should match unless maintainers agree and the exception is documented (code comment or `CLAUDE.md`).
- **Config on disk is not shared with Python:** use `~/.config/tuxtalks-oxide/config.json` (compatible key names; different path). Never read or write the Python app’s config dir from this crate at runtime.

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
- Run `make test` (or `cargo nextest run --all-features`) before pushing; CI uses **nextest**, not plain `cargo test`.
- **JRiver HTTP tests** rely on `wiremock` and set `TUXTALKS_NO_AUTOSTART=1` so failures never launch a real `mediacenter35`.
- **Voice D-Bus integration** (`tests/integration_dbus.rs`) is **opt-in**: set `TUXTALKS_OXIDE_DBUS_TESTS=1` and run with a session bus plus a live MPRIS player (e.g. VLC). Without that, those tests exit immediately (CI-safe).

### 3. Documentation

- All public items must have `///` doc comments.
- Prefer short, accurate comments in code; only add `/docs` material when the user or maintainers ask for a design note.

### Security

- Run `make audit` before merging dependency changes. Fix **vulnerabilities** (RustSec advisories marked as such); **warnings** (unmaintained / informational) may remain until upstream releases land — `cargo audit` exits 0 when only warnings are present.

### 4. Style

- Use `cargo fmt` before committing.
- Follow the patterns established in `src/lib.rs`.

## Development Workflow

1. Run `make check` to ensure correctness.
2. Run `make lint` to check style and lints.
3. Run `make test` to verify logic.
4. Run `make audit` for security check.
5. (Recommended) Run `claude` for a secondary logic review and test generation.
