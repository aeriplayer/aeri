# ADR 0024 — `aeri-player` decides what plays next in one pure value, the sequence

- **Status:** Accepted
- **Date:** 2026-11-16

## Context

[ADR 0005](0005-engine-plays-current-and-next.md) puts playlists, the queue, the order and stop-after-current above the engine, and [ADR 0006](0006-frontends-use-the-player-handle.md) puts the `Player` handle, an actor, in a new `aeri-player` crate. The actor adds the ordering of messages. What plays next is a decision over plain data, and an actor that interleaves it with engine calls would be hard to test: every case would need a thread and an engine.

Shuffle needs randomness. `fastrand` 2.5.0 (Apache-2.0 OR MIT, published 2026-07-19) was read on 2026-11-16: no build script, no dependencies on the targets aeri builds for, and `Rng::with_seed`, `Rng::shuffle` and `Rng::usize` need none of its cargo features. It is not cryptographic, which a shuffle does not need.

## Decision

- **A new crate, `aeri-player`.** This step gives it `aeri-core`, `fastrand` and `thiserror`. `aeri-engine` is added with the thread that uses it, because an unused dependency fails `cargo shear`. `deny.toml` is unchanged until then.
- **One value, `Sequence`, owns the playlists, the active playlist, the cursor, the order, the shuffle state, the queue and stop-after-current.** Its queries (`peek_next`, `peek_skip_next`, `peek_previous`) are pure and idempotent; its mutators (`take`, `jump_to`, the edits) are the only way the state changes. The actor calls them and decides nothing itself.
- **Ids are two counters, never reused.** A command that names an id that no longer exists returns an error. A position names an entry by playlist and entry id, as a struct, not a tuple.
- **A playlist is `Arc<Playlist>`, edited with `Arc::make_mut`.** An edit copies the list only while a published snapshot still holds it.
- **The cursor is the last entry taken from the order.** Taking a queued entry leaves it alone, so the order resumes where it was ([D6](../design/phase-4-playback.md)). Removing the cursor's entry moves it back one place, so the order continues with that entry's successor.
- **Natural advance and a user's skip differ.** Stop-after-current and repeat-track apply to the first only. `previous` goes to the entry before the cursor, or to the cursor itself when the current entry came from the queue.
- **Shuffle keeps two passes,** the one being played and the one after it, so the answer to "what next" at the end of a pass is known without drawing. An entry added mid-pass joins the unplayed part at a random place and the next pass at another. A removed one leaves both. The rng is owned by the sequence and seeded by the caller.
- **The first playlist created becomes the active one.**

## Consequences

- Every rule has a test that needs no thread, no engine and no clock. The actor's tests only have to show that it passes events and commands to the sequence in the right order.
- An edit to a playlist copies it while a snapshot is shared: linear in its length. Phase 5 measures this at 100,000 tracks.
- `previous` in shuffle goes back one place within the current pass and stops at its start; the previous pass is not kept.
- Switching the active playlist resets the cursor, so the order starts from that playlist's beginning, and the playing track is not part of it.
- The shuffle state exists only while the order is shuffle. Leaving shuffle and returning draws a new permutation.
- Moving an entry changes where the order continues from the cursor, because the cursor names an entry, not a place.
- Revisit if a second owner of playlists appears (the library phase), which would put this value behind a store rather than inside the player.
