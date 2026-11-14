# ADR 0008 — AAC and ALAC are in the v0.1 format floor; Opus is a C exception

- **Status:** Accepted
- **Date:** 2026-11-14

## Context

The format floor deferred AAC and ALAC "until stable symphonia support lands". The
pinned symphonia (0.6.1) already ships `aac`, `alac`, and `isomp4` features. Without
them a macOS player cannot open most iTunes-era libraries.

Separately, the project describes itself as pure Rust while Opus decodes through
`symphonia-adapter-libopus`, which links the libopus C library.

## Decision

- The v0.1 floor is: MP3, FLAC, Ogg Vorbis, Opus, WAV, AIFF, **AAC, ALAC** (the last
  two in MP4/M4A containers). The symphonia `aac`, `alac`, and `isomp4` features are
  enabled.
- Opus via libopus is the one sanctioned C dependency in the decode path. It is
  revisited when a pure-Rust Opus decoder is production-ready.

## Consequences

- Fixture files for round-trip and decode tests must include M4A (AAC and ALAC).
- AAC coverage is whatever symphonia provides; profiles beyond AAC-LC are to be
  confirmed with fixtures in Phase 5, not assumed. DRM-protected files are out of
  scope.
- "Pure Rust" in project descriptions carries the Opus caveat.
