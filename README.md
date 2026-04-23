# TuxTalks Oxide

Rust port of [TuxTalks](https://github.com/StarTuz/tuxtalks), a Linux voice assistant focused on media control. **CLI-first.** Voice is a build-time feature that's currently being reworked; the default binary is a media-control CLI.

## Status

- Default build: media CLI (`play`, `pause`, `status`, `next`, `previous`, `search`, `tracks`, `goto`, `albums`, `playlist`, `scan`, `check`).
- Backends: JRiver Media Center (MCWS HTTP), MPRIS (D-Bus), Strawberry and Elisa (SQLite).
- TTS: speechd-ng (primary) → `spd-say` (fallback) → logs.
- Voice assistant: `--features voice`, experimental, drifted from the Python reference and being reworked.

## Build

```bash
make ci                          # check + lint + test + audit (what CI runs)
cargo build                      # default: media CLI
cargo build --features voice     # include the experimental voice stack
cargo test                       # tests (default features)
cargo test --all-features        # include voice
cargo run -- status              # smoke-test
```

Requires a recent stable Rust toolchain (pinned via `rust-toolchain.toml`).

## Development & CI

GitHub Actions runs three jobs on `main` and PRs: **Check & Lint** (`cargo fmt` + `cargo clippy` with `clippy::pedantic`), **Test** (`cargo nextest run --all-features`), and **Security Audit** (`cargo audit`). Compile caching uses [Swatinem/rust-cache](https://github.com/Swatinem/rust-cache); we intentionally do **not** wrap `rustc` with `sccache`’s GitHub Actions backend, because when artifact cache is unavailable it can fail every compiler invocation (including `rustc -vV` during `rustup`).

Before opening a PR, run `make ci` locally — it matches what CI enforces.

### Environment variables for tests and scripts

| Variable | Effect |
|---|---|
| `TUXTALKS_NO_AUTOSTART` | When `1` / `true` / `yes` / `on`, JRiver autostart is disabled (JRiver integration tests set this so a mock HTTP flake cannot launch a real GUI). |
| `TUXTALKS_OXIDE_DBUS_TESTS` | When `1` / `true` / `yes` / `on`, runs the voice D-Bus integration tests in `tests/integration_dbus.rs` end-to-end (requires a session bus and a live MPRIS player such as VLC). **Unset in CI** — those tests short-circuit. |

Example (local workstation with VLC exposing MPRIS):

```bash
TUXTALKS_OXIDE_DBUS_TESTS=1 cargo nextest run --all-features -E 'test(::integration_dbus)'
```

### Security audit

`make audit` runs [`cargo-audit`](https://github.com/rustsec/rustsec) against [RustSec advisory-db](https://github.com/RustSec/advisory-db). **Vulnerabilities** fail the job; **warnings** (e.g. unmaintained crates, informational advisories) may still print but do not fail CI. After changing dependencies, run `cargo audit` and address any reported vulnerabilities (often a `cargo update -p …` to a patched version).

## Configuration

User config lives at `~/.config/tuxtalks-oxide/config.json`. Keys read by the Rust app:

| Key | Purpose |
|---|---|
| `PLAYER` | Default player (`jriver`, `strawberry`, `elisa`, `mpris`) |
| `JRIVER_IP`, `JRIVER_PORT`, `ACCESS_KEY` | JRiver MCWS |
| `JRIVER_BINARY` | JRiver executable for autostart (default `mediacenter35`) |
| `MPRIS_SERVICE` | e.g. `org.mpris.MediaPlayer2.vlc` |
| `STRAWBERRY_DB_PATH` | Strawberry's SQLite db |
| `LIBRARY_PATH` | Local music directory |
| `WAKE_WORD` | Voice-mode wake word |

Override via env (`JRIVER_IP=...`) or point at a different file with `TUXTALKS_OXIDE_CONFIG=/path/to/config.json`.

### JRiver autostart

If JRiver isn't running when you issue a command, the CLI will spawn `JRIVER_BINARY` and wait up to 20 seconds for it to come up (matching the Python app's behavior). Set `TUXTALKS_NO_AUTOSTART=1` to disable autostart — useful for CI, scripts, and anywhere a GUI shouldn't be launched.

### TTS backend

Set `TUXTALKS_TTS` to override the default auto-selection:

- unset / anything else → `auto` (speechd-ng → `spd-say` → tracing log)
- `speechd` / `speechd-ng` → force speechd-ng
- `spd-say` / `speech-dispatcher` → force `spd-say`
- `off` / `none` / `mute` → silent, log only

## Relationship to the Python app

This is a **separate project** from [`StarTuz/tuxtalks`](https://github.com/StarTuz/tuxtalks) (the Python original). The Rust port grows on its own schedule; the Python tree is a behavioral reference consulted when porting individual features. No code is shared at runtime; neither app reads the other's config files.

## License

See [`LICENSE`](LICENSE) — to be added.

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md) and [`TEAM_LEAD_GUIDELINES.md`](TEAM_LEAD_GUIDELINES.md).
