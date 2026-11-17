# ADR 0025 — The player is an actor with one inbox; slot offers are told apart by identity

- **Status:** Accepted
- **Date:** 2026-11-16

## Context

[ADR 0006](0006-frontends-use-the-player-handle.md) makes `Player` the one handle frontends use, and [ADR 0004](0004-engine-is-an-actor.md) makes long-lived owners actors. [ADR 0024](0024-aeri-player-sequence-is-pure.md) put every decision about what plays next in a pure `Sequence`; what is left is a thread that passes commands and engine events to it and keeps the engine's slot filled.

Four facts shape it:

- The engine's methods are `async`, and the player's thread has no runtime. Its inbox carries commands and engine events, and the thread waits on it with `blocking_recv`.
- The engine's event closure holds a sender to the inbox, so the inbox does not close when the last `Player` is dropped.
- An engine event names a track, not a slot offer. If the same `Arc<Track>` were offered twice, or a slot was replaced while the engine took the old one, an event could not be attributed.
- The engine reports a slot that cannot be opened only when the current track ends ([ADR 0023](0023-engine-events-and-dsp-chain.md)).

## Decision

- **One thread, `aeri-player`, and one unbounded inbox of `Message`: a command, an engine event, or shutdown.** `Message` implements `From<EngineEvent>`, so the engine is given the inbox's own sender.
- **The thread blocks on the engine's futures with a small `block_on` built on `std::task::Wake` and `thread::park`.** No runtime, no new dependency. The engine never waits on the player (its event sender is unbounded), so the two cannot deadlock.
- **Dropping the last handle sends `Shutdown`, then joins.** The thread leaves its loop and drops the engine, which joins its own thread.
- **Two watch channels.** `PlayerState` changes with edits and track changes. The engine's `PlaybackState` carries position and is passed through, so the player thread never wakes for a tick.
- **Every track given to the engine is a fresh `Arc` of a copy of the entry's track,** an *offer*. An event is matched to its offer by `Arc::ptr_eq`. The player keeps the offers the engine has not reported (`offers`), the ones it has taken but not yet played (`taken`), and the one being heard (`playing`). An event that matches none is stale, from before the last `play`, and is ignored.
- **The slot follows the sequence.** After every edit or event that can change `peek_next`, the player compares it with what it last offered and calls `set_next` only when they differ.
- **`current` follows `NowPlaying`.** A track is entered into the sequence on `TookNext`, so the cursor moves when the engine takes it and the heard track changes when the sound does.
- **Failures skip, with a bound.** A track that fails to start or to open counts one failure, and any track that plays resets the count. After as many consecutive failures as the active playlist has entries, the player stops and reports the error. A slot track that cannot be opened is found when the current one ends; the player starts the next one on `Ended`, with a gap.
- **`Ended` is checked against the engine's state.** If the engine is playing again, the event is stale.
- **`next` and `previous` with no such entry do nothing.** `play` from a stopped player starts the next entry the order names, or the first of the active playlist.
- **A `test-util` feature builds a player over `FakeSink` with a given shuffle seed.** The integration tests need it and are marked `required-features`.
- **`aeri-engine`'s wrapper in `deny.toml` becomes `aeri-player`, and `aeri-cli` moves onto `Player`.** The exception of ADR 0006 closes.

## Consequences

- A command is answered after the engine calls it makes. `play` opens the file, so a slow disk delays the replies queued behind it.
- A thread that blocks on futures with `park` is correct for tokio's channels, which wake without a runtime. It would not be for a future that needs a runtime's timers or I/O; none is awaited.
- The unreported-offer list is capped at 16. An offer older than that can be in flight only if the thread is that many messages behind, and the engine's `Failed` and `TookNext` for it would then be ignored as stale.
- A slot track that cannot be opened costs a gap, as ADR 0023 says.
- The shuffle seed outside tests comes from `RandomState`, which the standard library seeds from the operating system. It is not reproducible, and need not be.
- Seeking past the end of a track is the engine's error and leaves playback as it was.
- `previous` and `next` onto a track that fails to start with nothing playable after it leave the old track playing but the sequence's cursor on the failed one.
- Revisit if the engine's methods stop being `async`, which would remove the `block_on`.
