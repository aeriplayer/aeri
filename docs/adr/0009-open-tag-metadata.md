# ADR 0009 — Track metadata is an open, multi-value tag map; the schema waits for the library

- **Status:** Accepted
- **Date:** 2026-11-15

## Context

Two questions about metadata: what shape, and when to fix it.

**Shape.** A fixed struct (title, artist, album, …) cannot express what foobar-style players are used for: `%albumartist%`, `%discsubtitle%`, a user's own `%mood%` field, or three artists on one track. Title formatting (ADR 0010) addresses fields by name, including names aeri has never heard of.

**When.** The schema's consumers are the library database, the script language, and the GUI. Designed before any of them, it would be a guess that everything downstream hardens around.

## Decision

- A track's metadata is a map from a case-insensitive field name to **one or more** string values, plus typed technical properties read from the stream (duration, sample rate, channels, bit depth, bitrate, codec).
- A small set of well-known fields has typed accessors for speed and sorting; they are views over the map, not a second store.
- **The first-audio phase defines only what playback needs:** a minimal `Track` with a source, a duration, and optional title, artist, and album.
- The full type, its SQLite schema, and the mapping from each tag format are designed together at the start of the library phase.

## Consequences

- `Track` changes in the library phase. Accepted: nothing is stored before then.
- Field-name normalisation across ID3v2, Vorbis comments, and MP4 atoms is ours to define and is a compatibility surface once users write scripts against it. Follow foobar2000's field names where one exists.
- An open map costs memory and lookup time against a struct; ADR 0008's interning and the typed accessors are the mitigation, and the 100,000-track measurement is the check.
