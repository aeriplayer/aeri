# ADR 0005 — One SQLite writer, N WAL readers; no connection pool crate

- **Status:** Accepted
- **Date:** 2026-11-14

## Context

The library layer was specified on `deadpool-sqlite`. Two problems:

- It pinned `rusqlite` to 0.38 while rusqlite moved on (0.42 at the time of writing),
  making it the sole reason the database layer was held back.
- A pool is the wrong shape for SQLite. Writes serialise regardless of how many
  connections exist, so a pool of writers yields `SQLITE_BUSY`, not concurrency.

## Decision

`aeri-library` owns its connections directly on `rusqlite`:

- **One writer connection** on a dedicated thread, fed by an mpsc channel of write
  jobs. All mutations go through it, so write ordering is explicit and `SQLITE_BUSY`
  between our own connections cannot occur.
- **A small fixed set of read-only connections** in WAL mode, handed out for queries
  and run via `spawn_blocking`.

`deadpool-sqlite` is removed and `rusqlite` is unpinned.

## Consequences

- Scan upserts are naturally batched: the writer drains the channel into one
  transaction.
- The required PRAGMAs are applied in one place, at connection construction.
- We own roughly a hundred lines that a pool crate would have provided. In exchange
  the database layer no longer waits on a third party to track rusqlite.
- Read-your-writes is not automatic across connections; a caller that needs it awaits
  the write job's completion before querying.
