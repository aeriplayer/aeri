# ADR 0007 — Decoding is symphonia, FLAC and MP3 first; formats are added on demand

- **Status:** Accepted
- **Date:** 2026-11-15

## Context

fooyin decodes through FFmpeg plus native decoders and supports dozens of formats. Matching that means either linking FFmpeg, a large C dependency, or writing and testing many decoders before the first one has played. The owner's library is FLAC and MP3.

## Decision

- Decoding goes through the `Decoder` trait in `aeri-decode`, implemented over `symphonia`. No symphonia type appears in a public signature.
- **The supported formats are FLAC and MP3.** Only those symphonia features are enabled.
- A format is added when someone needs it, in its own change, with fixtures and a fuzz run. Formats symphonia supports are a feature flag and tests. Formats it does not support need a second `Decoder` implementation and an ADR for the dependency.
- **There is no C codec.** FFmpeg and libopus are not dependencies.

## Consequences

- aeri is Rust throughout; the only foreign code is the operating system's audio API.
- A user with Ogg, Opus, AAC, or ALAC files cannot play them until the format is added. Adding the ones symphonia covers is expected to be small; that expectation rests on symphonia's feature list (0.6.1 listed `aac`, `alac`, `isomp4`, `vorbis`, `ogg`, `wav`, `aiff` when checked on 2026-11-14), not on tested fixtures.
- Formats symphonia lacks are believed, from memory and unverified, to include WavPack, Monkey's Audio, Musepack, and Opus. Check before promising any of them.
- CUE sheets, archives, and remote streams are sources, not formats, and are out of this ADR's scope.
