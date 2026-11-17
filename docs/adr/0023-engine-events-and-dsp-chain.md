# ADR 0023 — The engine reports events on a caller-owned unbounded channel and runs the DSP chain

- **Status:** Accepted
- **Date:** 2026-11-16

## Context

[ADR 0004](0004-engine-is-an-actor.md) says events that must not be missed travel on their own channel, and [ADR 0005](0005-engine-plays-current-and-next.md) says the engine emits one on taking the next slot, and leaves open when a late change to the slot is too late. Phase 4 adds the owner that listens (`aeri-player`) and the DSP chain of [ADR 0020](0020-aeri-dsp-in-place-interleaved-chain.md), whose place in the pipeline is the engine's to decide.

The player has one inbox for its commands and the engine's events. If the engine handed back its own `Receiver`, the player thread would need two receivers and a `select!`, which means a tokio runtime on a thread that otherwise needs none.

## Decision

- **`Engine::spawn` takes an `mpsc::UnboundedSender<E>` where `E: From<EngineEvent>`.** The caller passes the sender of its own inbox. Only `spawn` is generic; the playback thread holds a boxed closure and `Engine` stays a plain type.
- **The sender is unbounded.** The playback thread never waits on a slow receiver. A send to a dropped receiver is logged at debug level and ignored.
- **Four events.**
  - `TookNext(track)`: the engine took the slot because the current decoder is exhausted, up to a ring's length before the change is heard. The slot is empty.
  - `NowPlaying(track)`: a track reached by taking the slot is now the one being heard. `play()` emits none; its reply says it started.
  - `Failed(track)`: a track could not be opened or failed while decoding, and was skipped. A slot track that fails to open was already taken, so the engine ends after the current track, and the owner continues on `Ended`.
  - `Ended`: playback stopped on its own, because the current track ended with the slot empty or because the output failed. An explicit `stop()` emits nothing. The engine's state already reads stopped when `Ended` is sent, so an owner can tell a stale one from a live one by looking at it.
- **A track that `TookNext` announced plays.** A later `set_next` fills the slot for the track after it. A seek during the wait for the output to drain before a reopen keeps the taken track, and it follows the current one when that is exhausted again, with no second `TookNext`.
- **The engine owns the DSP chain**, built from `Registry::builtin()` and `DEFAULT_CHAIN`. Each decoded block is copied into the engine's buffer, processed in place, then written. `start_track` is called when a track becomes the one being decoded, gapless or not, and `reset` wherever the sink is flushed or reconfigured. `set_volume` and `set_replay_gain` are commands, answered like the others.
- **A `test-util` feature re-exports `FakeSink`, its observer and `SinkCall`,** because `aeri-player`'s tests use the real engine over the fake and `deny.toml` forbids them to name `aeri-audio`. The engine also re-exports `aeri_decode::EXTENSIONS`.
- `aeri-engine` depends on `aeri-dsp`; `deny.toml` already allows exactly that.

## Consequences

- An owner whose receiver stalls lets events pile up without bound. They number a few per track, so this takes a stalled thread, not a long playlist.
- A volume change is heard after the audio already in the ring and the device buffer, about a quarter of a second ([ADR 0016](0016-real-time-bridge.md)). Tolerable on a held key is checked by hand in Step 10.
- A slot track that cannot be opened costs a gap: the engine ends, and the owner starts the track after it on `Ended`. Opening the slot's track earlier would remove it and is not done here.
- `Ended` for an output failure and for an end of playlist look alike. A failure also logs an error; an owner that must tell them apart needs a variant, and `EngineEvent` is `#[non_exhaustive]` for that.
- `NowPlaying` for a reopened output fires as the output reopens, not at the first audible frame. The ring is empty then, so the difference is milliseconds.
- A mid-decode failure emits `Failed` and moves on to the slot like a normal end. Tested only through its open-time form.
