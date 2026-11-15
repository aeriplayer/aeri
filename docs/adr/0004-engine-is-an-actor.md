# ADR 0004 — The engine is an actor; state fans out through a watch channel

- **Status:** Accepted
- **Date:** 2026-11-15

## Context

Playback state is changed by commands from several callers (the GUI, media keys, the
CLI) and by the decode loop itself. The obvious shape, a `Mutex<State>` shared across
threads, brings lock ordering, contention near the audio path, and the question of
what an observer sees and when.

## Decision

**The engine is an actor.** One dedicated thread owns all playback state. Callers hold
a cheap, cloneable handle and never touch that state.

- **Commands** are messages on an mpsc channel. Each carries a oneshot sender for its
  reply, so a caller awaits the outcome of its own command. Commands apply in arrival
  order; conflicting commands resolve as last-writer-wins.
- **State** is published on a `tokio::sync::watch` channel, written only by the
  playback thread. A watch channel holds one value, the latest: a new subscriber reads
  the current state at once, and a slow subscriber skips to the newest.
- **Events** that must not be missed travel on their own channel, never inside the
  state. The first is the engine's notice that it has moved to the next track
  (ADR 0005).

The same pattern applies to every long-lived owner of mutable state above the engine:
the player and the library.

## Consequences

- No lock guards playback state, so there is no lock ordering to get wrong and nothing
  for the real-time thread to contend on.
- No command may assume it knows the current state: "toggle pause" is resolved by the
  engine, not the caller.
- Position updates are throttled to about ten per second; subscribers interpolate.
- `tokio::sync` types appear in public APIs. They are runtime-independent: no tokio
  runtime runs on the playback thread or the audio thread, and the handle's `async`
  methods can be awaited from any executor, including GPUI's.
- Every command pays a channel round trip. That is irrelevant at human command rates
  and is the reason bulk data (track lists) is shared by `Arc`, not copied in messages.
