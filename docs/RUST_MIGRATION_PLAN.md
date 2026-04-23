# TuxTalks: Python to Rust Migration Plan

> **HISTORICAL DOCUMENT — kept for reference only.**
>
> This was the original aspirational plan when the Rust port was first scoped. It describes a full-feature lockstep migration that is **not** what the Rust port is doing anymore.
>
> The current reality is:
>
> - This repo **is** the Rust port, growing CLI-first on its own schedule.
> - The Python app lives at `~/Code/tuxtalks/` (GitHub: `StarTuz/tuxtalks`). It is a behavioral reference, not a migration target.
> - There is no "migration" in progress — there is a Rust port being written.
>
> See [`CLAUDE.md`](../CLAUDE.md) for the current framing and [`src/`](../src/) for what actually ships. The crate / tooling tables below are still useful reference when porting an individual feature.

## Executive Summary

Migrate TuxTalks (~25k lines Python) to Rust for:

- **Single binary distribution** - no venvs, no pip, no dependency hell
- **Performance** - faster startup, lower memory, no GIL
- **Native Linux integration** - zbus for D-Bus, evdev for input
- **Long-term maintainability** - type safety, no runtime errors

---

## Current Stack Analysis

### Python Dependencies → Rust Equivalents

| Python | Purpose | Rust Crate | Maturity |
|--------|---------|------------|----------|
| `vosk` | Offline ASR | `vosk` | ✅ Stable |
| `pyaudio` | Audio capture | `cpal` | ✅ Stable |
| `dbus-python` | D-Bus IPC | `zbus` | ✅ Excellent |
| `pynput` | Key simulation | `evdev` + `uinput` | ✅ Native |
| `evdev` | Input events | `evdev` | ✅ Same |
| `ttkbootstrap` | GUI | `iced` or `egui` | ✅ Mature |
| `wyoming` | Wyoming protocol | Custom impl | ⚠️ Port needed |
| `requests` | HTTP | `reqwest` | ✅ Stable |
| `psutil` | Process info | `sysinfo` | ✅ Stable |
| `defusedxml` | XML parsing | `quick-xml` | ✅ Stable |

### Codebase by Module

| Module | Lines | Rust Notes |
|--------|-------|------------|
| `game_manager.py` | 2,503 | Core logic, straightforward port |
| `launcher_games_tab.py` | 2,945 | GUI, depends on toolkit choice |
| `tuxtalks.py` | 831 | Main loop, async with `tokio` |
| `command_processor.py` | 763 | State machine, clean port |
| `voice_fingerprint.py` | 476 | Serde JSON handling |
| `speech_engines/*` | ~600 | cpal + vosk/whisper-rs |
| `parsers/*` | ~400 | quick-xml |

---

## Architecture Proposal

```
┌─────────────────────────────────────────────────────────────┐
│                      tuxtalks-rs                             │
│  ┌─────────────────┐  ┌─────────────────┐  ┌──────────────┐ │
│  │   Audio Engine  │  │  Command Engine │  │  Game Engine │ │
│  │   cpal + rodio  │  │  State Machine  │  │  XML Parsers │ │
│  └────────┬────────┘  └────────┬────────┘  └──────┬───────┘ │
│           │                    │                   │         │
│           ▼                    ▼                   ▼         │
│  ┌─────────────────────────────────────────────────────────┐│
│  │                     Core Runtime (tokio)                 ││
│  │  - ASR: vosk-rs / whisper-rs                            ││
│  │  - Input: evdev + uinput (native, no pynput)            ││
│  │  - IPC: zbus (D-Bus, speechd-ng integration)            ││
│  └─────────────────────────────────────────────────────────┘│
│                              │                               │
│                              ▼                               │
│  ┌─────────────────────────────────────────────────────────┐│
│  │                       GUI Layer                          ││
│  │         iced (Elm architecture) or egui                  ││
│  └─────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────┘
```

---

## Key Benefits

### 1. Native D-Bus with zbus

```rust
#[dbus_proxy(interface = "org.speech.Service")]
trait SpeechService {
    fn speak(&self, text: &str);
    fn listen_vad(&self) -> String;
}
```

No Python bindings, no FFI overhead, async-native.

### 2. Native Input with evdev

```rust
// Direct kernel evdev - works on Wayland without hacks
let device = uinput::Device::create()?;
device.key(Key::KEY_A).press()?;
device.key(Key::KEY_A).release()?;
```

No pynput, no X11 dependency, works everywhere.

### 3. Single Binary Distribution

```bash
# Build
cargo build --release

# Package - just one file
cp target/release/tuxtalks /usr/local/bin/

# No venv, no pip, no python
```

### 4. Wyoming Protocol (Custom Port)

The `wyoming` Python package is simple enough to reimplement:

- TCP socket with JSON-ish events
- AudioStart/AudioChunk/AudioStop/Transcript
- ~200 lines of Rust

---

## Migration Strategy

### Phase 1: Core Runtime (Weeks 1-2)

- [ ] Set up Cargo workspace
- [ ] Implement audio capture with `cpal`
- [ ] Implement ASR with `vosk-rs`
- [ ] Implement basic command processing
- [ ] Test: Voice → text → action works

### Phase 2: Input & IPC (Week 3)

- [ ] Implement key simulation with `evdev`/`uinput`
- [ ] Implement D-Bus client with `zbus`
- [ ] speechd-ng integration via D-Bus
- [ ] Test: Voice commands trigger keypresses

### Phase 3: Game Integration (Week 4)

- [ ] Port XML parsers (`quick-xml`)
- [ ] Port game_manager logic
- [ ] Port macro execution
- [ ] Test: Elite/X4 bindings work

### Phase 4: GUI (Weeks 5-6)

- [ ] Evaluate `iced` vs `egui`
- [ ] Port launcher UI
- [ ] Port games tab (most complex)
- [ ] Port settings/config

### Phase 5: Polish (Week 7)

- [ ] CLI entrypoints
- [ ] Packaging (deb, rpm, AUR)
- [ ] Documentation
- [ ] Migration guide for users

---

## GUI Toolkit Recommendation

| Feature | iced | egui |
|---------|------|------|
| Architecture | Elm (reactive) | Immediate mode |
| Learning curve | Steeper | Easier |
| Customization | Excellent | Good |
| Native look | Custom theming | Custom theming |
| Maturity | Very active | Very active |
| Wayland | ✅ Native | ✅ Native |

**Recommendation: `iced`** for TuxTalks because:

- Elm architecture fits well with state-driven UI
- Better for complex multi-tab layouts
- More "desktop app" feel

---

## Risk Assessment

| Risk | Mitigation |
|------|------------|
| GUI complexity | Keep Python launcher initially, port core first |
| Wyoming protocol | Simple protocol, easy to reimplement |
| Learning curve | Existing speechd-ng Rust codebase as reference |
| Build dependencies | Static linking where possible |
| User migration | Ship alongside Python version initially |

---

## Verification Plan

1. **Unit tests** with `cargo test`
2. **Integration tests** - voice → action pipeline
3. **Game tests** - ED/X4 bindings parse correctly
4. **Performance benchmarks** vs Python version
5. **Packaging tests** - deb/rpm install cleanly

---

## Timeline Estimate

| Phase | Duration | Deliverable |
|-------|----------|-------------|
| Core Runtime | 2 weeks | Voice → text → key works |
| Input & IPC | 1 week | D-Bus + evdev integration |
| Game Integration | 1 week | ED/X4 profiles work |
| GUI | 2 weeks | Full launcher replacement |
| Polish | 1 week | Packaged releases |
| **Total** | **7 weeks** | Production-ready |

---

## Next Steps

1. Create `tuxtalks-rs` cargo workspace
2. Start with audio + ASR core
3. Keep Python version running in parallel
4. Migrate users once feature parity achieved
