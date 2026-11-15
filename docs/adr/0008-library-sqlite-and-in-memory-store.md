# ADR 0008 — The library persists to SQLite and is queried in memory

- **Status:** Accepted
- **Date:** 2026-11-15

## Context

In a foobar-style player a playlist column, a grouping, a filter, a search, and an autoplaylist are all expressions in one script language (ADR 0010), evaluated per track over arbitrary tags.

SQLite could be the query engine, with FTS5 for search and paginated SQL for browsing. But script expressions cannot be pushed into SQL without compiling the language to SQL, and FTS5 answers only one of those questions. fooyin loads every track into memory and evaluates there.

## Decision

- **SQLite is the persistence format**, on `rusqlite` with bundled SQLite, in WAL mode. It stores tracks, tags, playlists, statistics, and settings.
- **One connection on one dedicated thread** owns every write, fed by a channel of write jobs. SQLite serialises writes regardless, so more writers would buy `SQLITE_BUSY`, not concurrency. No pool crate.
- **The query surface is an in-memory track store.** At start-up the library loads every track into an immutable snapshot shared by `Arc`. A scan or tag edit produces a new snapshot and publishes it on a watch channel (ADR 0004). Filtering, sorting, grouping, and search run over the snapshot.
- **No FTS5 and no paginated SQL.**

## Consequences

- Readers never block the writer and never see a half-applied scan: they hold a snapshot.
- Memory grows with the library. Repeated strings (artist, album, genre) are interned. The budget is 100,000 tracks in a few hundred megabytes; measure it in the library phase with a generated library, before the schema is fixed.
- Start-up reads the whole track table. Measure that too; the fallback is a cached binary snapshot, not lazy loading.
- A full-library query is a linear scan per keystroke. It parallelises with `rayon` and should be measured at 100,000 tracks before any index is considered.
- Revisit if either measurement fails at 100,000 tracks.
