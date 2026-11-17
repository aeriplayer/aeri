# ADR 0026 — The callback counts an underrun only once audio has played since the last flush

- **Status:** Accepted; refines the `underruns` counter of [ADR 0016](0016-real-time-bridge.md)
- **Date:** 2026-11-17

## Context

[ADR 0016](0016-real-time-bridge.md) has the callback count every buffer it could not fill, and the sink logs the count as "audio output ran dry". A flush (seek, skip, a new track) empties the ring on purpose, and the callback can run before the decoder has refilled it. Listening on macOS with the Phase 4 keys, every skip and seek logged one such warning and nothing else did: the count could not tell a flush from a decoder that fell behind, so the warning was true but not useful.

The callback already holds what separates them. After a flush with nothing new written, its read position equals `flush_at`; once audio has played since, it is past it.

## Decision

- The callback counts an underrun only when its read position, after it has discarded flushed audio and before it consumes any, is past `flush_at`: audio has played since the last flush, or since the stream opened.
- The first buffer after a flush or a stream start is therefore never an underrun, even if it is only partly filled. The engine already reports that state as `Buffering`.
- It adds one comparison to the callback and no state, atomic, allocation, or lock. The protocol of ADR 0016 is unchanged.

## Consequences

- A skip or seek no longer logs a warning.
- A stall before the first frame after a flush, such as a very slow decode after a seek, is not reported. A stall once audio is playing still is.
- A buffer that is only partly filled right after a flush plays its silence unreported.
- Revisit if a report of stalls during `Buffering` is wanted; it would need to be a separate counter, not a change to this one.
