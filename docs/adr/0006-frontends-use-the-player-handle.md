# ADR 0006 — Frontends use one concrete `Player` handle; platform integrations are frontends

- **Status:** Accepted
- **Date:** 2026-11-15

## Context

The GUI, the CLI, MPRIS, and the macOS Now Playing centre all issue transport commands and display playback state. If each reaches into the engine, the library, and the playlists separately, they can disagree, and the application has no single surface to test.

That surface could be a trait or a concrete type. aeri is one process (ADR 0001), so a trait would have one implementor.

## Decision

`aeri-player` exposes a concrete, cloneable `Player` handle. It is an actor (ADR 0004) that owns the engine, the library, and the playlists.

- **Every frontend depends on `aeri-player` and on nothing beneath it.** `aeri-gui`, `aeri-cli`, and `aeri-platform` never name `aeri-engine`, `aeri-audio`, `aeri-decode`, `aeri-dsp`, or `aeri-library`. `deny.toml` enforces this.
- **`Player` is a struct, not a trait.** A trait with one implementation is indirection that buys nothing; tests use the real `Player` over `FakeSink`.
- **Platform integrations are frontends.** `aeri-platform` is written against `Player` and is started by the binary, never by the engine.

Until `aeri-player` exists (the playback phase), `aeri-cli` depends on `aeri-engine` directly. That exception ends when the crate is created.

## Consequences

- Media keys and the GUI go through one code path, so they cannot disagree.
- The engine has no OS-integration code and stays testable headless.
- `Player`'s API is the application's contract and will be large. It is kept honest by being the only thing frontends can call.
- If a second implementation is ever needed, extracting a trait from a concrete type with known callers is mechanical. Guessing the trait first is not.
