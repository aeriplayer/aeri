# ADR 0006 — Platform integrations are `Player` clients, not engine internals

- **Status:** Accepted
- **Date:** 2026-11-14

## Context

MPRIS and the macOS MediaPlayer remote command centre *issue* transport commands and
*display* now-playing state. By this project's own definition that makes them
frontends. They were nevertheless wired inside `aeri-engine`, and `aeri-platform`
depended only on `aeri-core`, so it had no way to call back into playback at all.

## Decision

`aeri-platform` depends on `aeri-ipc` (without the `local` feature) and is written
against the `Player` trait: it sends commands through it and renders the state
subscription from ADR 0001.

It is started by whichever process hosts the engine — the daemon in `aeri-cli`, or
`aeri-gui` in a single-binary build. `aeri-engine` no longer depends on
`aeri-platform`.

## Consequences

- The engine has no OS-integration code; it stays testable headless.
- Media keys and the GUI go through one code path, so they cannot disagree.
- `aeri-cli` and `aeri-gui` gain a dependency on `aeri-platform`. A remote-only client
  must not start it: the integration belongs to the machine producing the audio.
- Platform integration cannot land before the `Player` trait and its state
  subscription exist (Phase 3), which matches its Phase 5 slot.
