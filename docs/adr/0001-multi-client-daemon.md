# ADR 0001 — The daemon is multi-client

- **Status:** Accepted
- **Date:** 2026-11-14

## Context

The roadmap listed "single-client vs. multi-client" as a Phase 2 blocker. The question
is already answered by the rest of the design: MPRIS / the macOS remote command centre,
the CLI, the GUI, and a future phone client all talk to one engine, and at least two of
them are connected at the same time in ordinary use.

## Decision

The daemon serves any number of concurrent clients. There is no client ownership,
locking, or session concept for playback.

- **Commands** from all clients funnel into the engine's single command queue and are
  applied in arrival order. Conflicting commands resolve as last-writer-wins.
- **State** is observed, not polled. The `Player` trait exposes a subscription to
  playback state changes, and every client — including an in-process `LocalPlayer`
  caller — uses it. On the wire this becomes a server-streaming RPC.

## Consequences

- The `Player` trait must have the subscription method from its first version
  (Phase 3), even though only `LocalPlayer` exists then. Adding it later would mean
  rewriting every frontend that grew up polling.
- No command may assume it knows the current state; e.g. "toggle pause" is resolved by
  the engine, not the client.
- The remaining design work is the fan-out itself (slow subscribers, initial snapshot
  on subscribe). That is done when the wire protocol lands — see ADR 0002.
