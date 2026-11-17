# ADR 0027 — The engine opens the slot's file when it is set, not when the current track ends

- **Status:** Accepted; refines [ADR 0023](0023-engine-events-and-dsp-chain.md)
- **Date:** 2026-11-17

## Context

[ADR 0023](0023-engine-events-and-dsp-chain.md) had the engine open the slot's file only when the current decoder was exhausted. A file that could not be opened was found then, too late: the engine had nothing to play, ended, and the player started the next entry on `Ended`. Listening to a playlist with an unplayable file in the middle on macOS, that was a gap of about 0.2 s and a warning that the output ran dry. ADR 0023 listed that gap as a cost.

## Decision

- **`set_next` opens the file at once** and keeps the decoder in the slot with its track. Taking the slot at the end of the current track then needs no I/O.
- **A file that cannot be opened is reported at once:** `EngineEvent::Failed` is sent when `set_next` is handled, the slot is left empty, and the current track keeps playing. `set_next` itself still succeeds.
- **The player refills the slot on `Failed`.** It passes the failed entry over, and offers the next one, until one opens or as many have failed in a row as the playlist has entries. A bad file in the middle of a playlist is then skipped without a gap.
- `TookNext` and the rest of ADR 0023 are unchanged. `Failed` for a track that fails while decoding is unchanged too.

## Consequences

- The probe runs on the playback thread, between decode steps, so a slow disk can delay a block. The ring holds 200 ms and the same open used to happen at the boundary, so the exposure is not new, only earlier.
- A decoder and its buffers are held for the slot's track for as long as it waits. That is one file's worth.
- A file that opens but is corrupt further in still fails while decoding, at the time it plays, and still costs the rest of that track.
- Replacing or clearing the slot drops the decoder it held.
- A file that changes between being set and being taken is read as it was opened.
