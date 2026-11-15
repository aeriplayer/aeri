# ADR 0005 — The engine plays a current track and a next slot; playlists live above it

- **Status:** Accepted
- **Date:** 2026-11-15

## Context

A foobar-style player decides what plays next from several sources at once: the active
playlist, the playback order (default, repeat, shuffle), an explicit queue that
overrides the order, and "stop after current". fooyin keeps all of that in its player
controller and gives its audio engine only a track to play and a track to prepare.

An engine that owned the queue would have to learn every one of those rules, and each
new rule would be an engine change.

## Decision

`aeri-engine` knows two tracks: the one playing and an optional **next**.

- `play(track)` starts a track now. `set_next(Option<Track>)` fills or clears the slot.
- When the current track ends, the engine takes the slot. If it holds a track with the
  same stream spec, playback continues gaplessly (ADR 0003); if the slot is empty,
  playback stops.
- On taking the slot the engine emits an event (ADR 0004). The owner responds by
  choosing the following track and calling `set_next` again.

Playlists, the queue, playback order, and history belong to `aeri-player`, which owns
the engine and is the only caller of `set_next`.

## Consequences

- The engine has no notion of playlist, index, or order, and is tested with two
  tracks.
- `aeri-core` carries no queue type in the first-audio phase; playlist types arrive
  with `aeri-player`.
- The player must refill the slot before the new current track ends. A track shorter
  than that round trip produces a gap. Accepted: the round trip is a channel hop.
- Changing the next track late (the user reorders the playlist during the last
  seconds) may arrive after the engine has begun decoding the old choice. The engine
  discards pre-decoded audio it has not yet written to the sink; audio already in the
  sink plays. The exact cut-off is decided with the tests in the playback phase.
- Crossfade needs two streams decoding at once. The slot is where the second stream
  comes from, so crossfade extends this design and does not replace it.
