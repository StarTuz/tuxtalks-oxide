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

## Configuration

User config lives at `~/.config/tuxtalks-oxide/config.json`. Keys read by the Rust app:

| Key | Purpose |
|---|---|
| `PLAYER` | Default player (`jriver`, `strawberry`, `elisa`, `mpris`) |
| `JRIVER_IP`, `JRIVER_PORT`, `ACCESS_KEY` | JRiver MCWS |
| `MPRIS_SERVICE` | e.g. `org.mpris.MediaPlayer2.vlc` |
| `STRAWBERRY_DB_PATH` | Strawberry's SQLite db |
| `LIBRARY_PATH` | Local music directory |
| `WAKE_WORD` | Voice-mode wake word |

Override via env (`JRIVER_IP=...`) or point at a different file with `TUXTALKS_OXIDE_CONFIG=/path/to/config.json`.

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
