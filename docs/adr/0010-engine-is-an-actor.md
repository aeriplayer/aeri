# ADR 0010 — The engine is an actor; state fans out through a watch channel

- **Status:** Accepted
- **Date:** 2026-11-14
- **Resolves:** the "state fan-out" open decision left by ADR 0001

## Context

ADR 0001 made the daemon multi-client and required that state be subscribed to, not
polled, leaving two questions open: what a new subscriber sees first, and what happens
to a subscriber that reads slowly. Separately, the engine's playback state is touched
by commands from any number of callers and by the decode loop itself, which invites a
`Mutex<State>` shared across threads.

## Decision

**The engine is an actor.** One dedicated thread owns all playback state. Callers hold
a cheap, cloneable handle and never touch that state directly.

- **Commands** are messages on an mpsc channel. Each carries a oneshot sender for its
  reply, so a caller awaits the outcome of its own command.
- **State** is published on a `tokio::sync::watch` channel, written only by the
  playback thread.

A watch channel holds exactly one value, the latest. That answers both open questions:

- A new subscriber reads the current state immediately — the initial snapshot is free.
- A slow subscriber skips intermediate values and sees the newest. It cannot fall
  behind, cannot make the engine wait, and cannot grow a queue.

## Consequences

- No lock guards playback state, so there is no lock ordering to get wrong and nothing
  for the real-time thread to contend on.
- Subscribers observe *states*, not *events*. Anything that must not be missed (for
  example "track finished", for scrobbling) is not expressible as a watch update and
  needs its own channel when that need arrives. Do not smuggle events into the state.
- Position updates are throttled by the engine (about ten per second); subscribers
  interpolate if they want smoother display.
- On the wire (Phase 4) the subscription becomes a server-streaming RPC that forwards
  watch updates. `RemotePlayer` feeds a local watch channel from that stream so both
  `Player` implementations hand out the same type.
- `tokio::sync` types appear in `aeri-engine` and in the `Player` trait. They are
  runtime-independent primitives; this does not put a tokio runtime on the playback
  thread, and never on the audio thread.
