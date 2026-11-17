# ADR 0030 — aeri changes nothing the user did not ask it to change

- **Status:** Accepted
- **Date:** 2026-11-17

## Context

aeri is a foobar2000-class player with a larger aim: a library the user curates over years ([`README.md`](../../README.md)). Curation means aeri will eventually be able to write tags, rename and move files, and fix what it finds wrong. A player that does those things on its own initiative, such as rewriting tags on import, reorganising folders, or "correcting" metadata, is the behaviour that drives people to foobar2000 in the first place. Nothing in the code does it yet, so the rule is cheapest to set now.

## Decision

- aeri never modifies a user's audio files, tags, or directory layout unless the user asked for that specific change. Scanning, indexing, playing, and analysing are read-only on the files.
- Every operation that does write to files is an explicit action, previews what it will change before it changes anything, and can be run as a dry run from the command line. A change is never a side effect of another action.
- Anything aeri derives from files, such as normalised tag names, a computed ReplayGain, or a fingerprint, is stored in its own database, not written back to the files.
- Defaults must obey this too: a default may suggest a change, never make one.

## Consequences

- Features that would be smooth if they wrote silently (ReplayGain scans, tag clean-up, rename on import) cost an explicit step. That is the cost accepted.
- The library must be correct for files it has not changed, so it needs its own store for derived data ([ADR 0008](0008-library-sqlite-and-in-memory-store.md)).
- Writing code (tag editing, file operations) needs its own design with the preview, the dry run, and what happens on a crash part-way. This ADR does not design them.
- Not verified: that every future dependency, such as a tag library, is read-only until asked. Check this when one is added.
- Revisit only if the user, not a default, wants an automatic behaviour, and then as an opt-in setting.
