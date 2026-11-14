# ADR 0009 — The metadata schema is designed in Phase 4, not Phase 2

- **Status:** Accepted
- **Date:** 2026-11-14

## Context

Phase 2 was blocked on finalising the full metadata schema (tracks, albums, artists,
tags, custom fields) before any code existed. The schema's real consumers are the
library database, the browse/search RPCs, and the GUI — all Phase 4 or later. Designed
with no consumer, it would be a guess that everything downstream then hardens around.

## Decision

Phase 2 defines only what playback needs:

- `PlaybackState` — stopped, playing, paused, buffering; position and duration.
- `Queue`.
- A minimal `Track` — source location, duration, and optional display strings
  (title, artist, album).

The metadata schema is designed at the start of Phase 4, alongside the SQLite schema
that stores it and the RPCs that carry it.

## Consequences

- `Track` will change in Phase 4. That is accepted: there are no users and no stored
  data before then.
- The IPC contract "a track row is renderable without a follow-up call" applies from
  Phase 4, when library RPCs exist.
- Phase 2 has no blocker and is small enough to fold into the start of Phase 3 work.
