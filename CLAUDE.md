# CLAUDE.md

Guidance for Claude Code (claude.ai/code) when working in this repository.

## What this repo is

**TuxTalks Oxide** — a Rust reimplementation of the TuxTalks voice assistant, growing **CLI-first**.

- Single-crate repo, flat layout. `Cargo.toml` is at the root.
- Default `cargo build` produces a media-control CLI. A `voice` cargo feature gates an experimental voice stack.
- Not a 1:1 mirror of the Python original and not trying to be. Feature parity is *per-feature when ported*, not holistic.

## Python reference (separate repo)

The Python app is **a separate project at `~/Code/tuxtalks/`** (GitHub: `StarTuz/tuxtalks`). It is **not** in this workspace.

Use it as a behavioral reference when porting a specific feature:

- `players/jriver.py` — MCWS HTTP shape, XML field names, `Playback/Info` semantics
- `core/text_normalizer.py` — number-word parsing, alias application, wake-word stripping
- `core/command_processor.py` — voice intent routing (the model the Rust voice rework will match)
- `speech_engines/speechd_ng_tts.py` — speechd-ng client patterns (Rust now uses the D-Bus `org.speech.Service` interface directly)

When porting a feature, **match Python's behavior for that feature**. When not porting, don't. The Rust app is allowed to be smaller.

## Commands

```bash
make ci              # check + lint + test + audit (what CI runs)
make check           # cargo check --all-targets
make test            # cargo nextest run --all-features
make lint            # cargo fmt --check + cargo clippy --pedantic -D warnings
make audit           # cargo audit

cargo build                       # default: media CLI only
cargo build --features voice      # include the (drifted) voice stack
cargo test                        # default tests
cargo test --all-features         # include voice
cargo run -- status               # smoke-test the CLI
```

## Architecture

```
main.rs (clap CLI)
  └── PlayerManager  (players/manager.rs)
        ├── JRiverPlayer     → HTTP (reqwest) + MCWS XML (quick-xml)
        ├── MprisPlayer      → D-Bus (zbus)
        ├── StrawberryPlayer → SQLite (rusqlite) + MPRIS
        └── ElisaPlayer      → SQLite (rusqlite) + MPRIS

MediaPlayer trait (lib.rs) — all players implement this
Speaker (utils/speaker.rs) — mpsc producer; worker routes → speechd-ng → spd-say → log
```

### Key modules

- `src/lib.rs` — `MediaPlayer` trait, `PlayerError`, newtypes (`Artist`, `Album`, `Track`, `Genre`), `NowPlaying`, `SelectionItem`, `SearchResult`
- `src/config.rs` — `PlayerConfig` (env + file), `PlayerContext` (shared `Arc` state)
- `src/players/*.rs` — backend impls
- `src/utils/speaker.rs` — TTS producer + worker (speechd-ng primary, `spd-say` fallback)
- `src/utils/fuzzy.rs` — Jaro-Winkler fuzzy matching with tiered scoring
- `src/utils/text_normalize.rs` — pure-Rust port of `core/text_normalizer.py`
- `src/utils/library.rs` — SQLite-backed local media library
- `src/utils/circuit_breaker.rs` — Circuit breaker for network-ish players

### Feature-gated (voice only)

`src/active_loop.rs`, `src/integration/`, `src/intelligence/`, voice CLI subcommands (`listen`, `add-correction`, `daemon`), and `tests/integration_dbus.rs`. Currently an LLM-based experimental path that drifted from Python's proven pipeline; being reworked.

## Configuration

Single JSON file with Python-compatible keys (same field names so values can be copied over):

- User config: `~/.config/tuxtalks-oxide/config.json`
- Local dev: `./tuxtalks-oxide.config.json` (cwd), then `${CARGO_MANIFEST_DIR}/tuxtalks-oxide.config.json`
- Explicit: `TUXTALKS_OXIDE_CONFIG` or `TUXTALKS_CONFIG` env var
- Library DB default: `~/.local/share/tuxtalks-oxide/library.db`

Keys Rust reads: `PLAYER`, `JRIVER_IP`, `JRIVER_PORT`, `ACCESS_KEY`, `JRIVER_BINARY`, `MPRIS_SERVICE`, `STRAWBERRY_DB_PATH`, `LIBRARY_PATH`, `WAKE_WORD`. Env equivalents: same names, or `JRIVER_`-prefixed.

**JRiver autostart** (Python parity with `players/jriver.py::health_check`): when any JRiver call hits `connection refused`, the CLI spawns `JRIVER_BINARY` (default `mediacenter35`) and polls `/Alive` for up to 20 s. Disable with `TUXTALKS_NO_AUTOSTART=1` (also auto-enabled by the JRiver integration tests so `wiremock` flakes can't spawn a real GUI).

**Never** read `~/.config/tuxtalks/` (that's the Python app's config dir; leave it alone).

### TTS backend selection (`TUXTALKS_TTS` env var)

- unset / anything else → `auto` (speechd-ng → spd-say → tracing log)
- `speechd` / `speechd-ng` → speechd-ng only, drop on failure
- `spd-say` / `speech-dispatcher` → spd-say only
- `off` / `none` / `mute` → silent, log only

## Rust code standards

- No `unwrap()` / `expect()` outside tests and `main.rs` boilerplate.
- All code targets `clippy::pedantic` with zero warnings. `make lint` enforces this.
- Network players (JRiver) and SQLite access go through the circuit breaker.
- Newtype wrappers for domain values (`Artist(String)`, not raw `String`).
- Fuzzy matching and intent parsing have `proptest` coverage.
- Integration tests for HTTP players use `wiremock`; D-Bus tests are voice-gated.
- Errors surface the real cause chain — when wrapping `reqwest::Error`, walk `source()` so "connection refused" isn't swallowed (see `describe_reqwest_error` in `players/jriver.rs`).

## When porting a feature from Python

1. Read the Python function(s) at `~/Code/tuxtalks/`. Note XML/JSON shapes, error cases, user-visible strings.
2. Match behavior for the feature being ported, not the full module. If Python's `go_to_track` loops `Next`/`Previous`, so does the Rust one — even when MCWS has a cleaner primitive, matching Python catches drift. Document deviations (different library, async model, etc.) in a comment at the function.
3. Keep scope tight. If the Python function depends on 3 other helpers, port only what you need for the current slice; stub the rest.
4. Add a `wiremock` / unit / proptest test that fails without your change. Cite the Python reference (`// Mirrors players/jriver.py::go_to_track`) in the test or the impl.
5. Update `src/main.rs` (CLI wiring) and the CLI help text if the feature is user-reachable.

## When **not** porting a feature

Don't. The Rust app is allowed to be smaller. Pick the next slice that makes the CLI more useful for the user's actual workflow.

## Working rules

- This repo contains Rust only. Do not copy Python files in.
- Don't read or write `~/Code/tuxtalks/` or `~/.config/tuxtalks/` at runtime — those belong to the Python app.
- `docs/RUST_MIGRATION_PLAN.md` is **historical** — a record of original aspirations, not a current plan.
