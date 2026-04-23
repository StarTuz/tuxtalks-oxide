# TuxTalks Oxide: Team Lead Standards of Excellence

This document defines the "Claude 4.6" standard of excellence for the TuxTalks Oxide Rust port. Every contributor (human or AI) must adhere to these principles.

## 0. The Gold Standard (Python Reference)

- **Source of truth (behavior only)**: The Python app at `~/Code/tuxtalks/` (GitHub: `StarTuz/tuxtalks`) is a **separate project** that defines correct **behavior** for features we port — not Rust idioms or stack choices. Logic, workflows, and semantics come from there. The Python tree is **not** in this repo and **not** edited from this repo.
- **No Assumptions**: Do not assume implementation details or add "extra" features unless explicitly requested.
- **Reference-First**: Always refer back to the Python source code when implementing or verifying Rust functionality to ensure behavioral parity.

## 1. Architectural Integrity (Architect & Team Lead)

- **Zero-Cost Abstractions**: Use traits and generics to ensure no runtime penalty for flexibility.
- **Fail-Fast Interfaces**: Use `Result` and `Option` aggressively. No `unwrap()` or `expect()` outside of tests or `main.rs` boilerplate.
- **Circuit Breaker Pattern**: Any network-based player (JRiver) or shared resource (SQLite) must implement circuit breaking to prevent UI/Voice hangs.

## 2. Code Quality & Safety (Security & QA)

- **Newtype Everything**: Protect domain logic with Newtype wrappers (e.g., `MsDelay(u32)`).
- **Clippy Pedantic**: All code must pass `clippy::pedantic` without warnings.
- **Property-Based Verification**: Fuzzy matching and command parsing MUST be verified with `proptest`.

## 3. The "Claude Review" Workflow

Before any major logic is merged into the media control layer (Oxide):

1. **Lint & Test**: Run `make check` and `make test`.
2. **Claude Audit**: Invoke `claude "Review [files] for architectural consistency and potential race conditions."`.
3. **Snapshot Update**: If logic changes, update `insta` snapshots explicitly.

## 4. Media Control Boundary

- **Oxide is a CONTROL layer**: We do not implement playback; we implement the bridge.
- **Prioritize "Active"**: Always prioritize the active/playing player via MPRIS discovery.

---
*Signed,*
**Claude 4.6 (Team Lead)**
