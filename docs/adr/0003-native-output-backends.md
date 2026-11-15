# ADR 0003 — Native output backends; bit-perfect and gapless claims are scoped

- **Status:** Accepted
- **Date:** 2026-11-15

## Context

`cpal` is the usual Rust audio-output abstraction. On Linux it reaches devices through
ALSA; on macOS it does not expose hog mode or device sample-rate switching, which
bit-perfect playback needs. It abstracts over exactly the parts aeri must reach
beneath.

Two claims a player likes to make also need limits:

- Bit-perfect output assumes the application controls the device rate. Under PipeWire
  the server owns the graph rate; a stream may request a rate, and whether the graph
  switches is server policy.
- Gapless playback cannot hold across a device re-initialisation, which a change of
  sample rate between tracks requires.

## Decision

1. **No cpal.** `OutputSink` has two native implementations in `aeri-audio`:
   `CoreAudioSink` and `PipeWireSink`, selected by `cfg(target_os)`. Tests use
   `FakeSink`. There is no PulseAudio, ALSA, or JACK backend.
2. **Bit-perfect is guaranteed on macOS only**, in hog mode with the device switched to
   the source rate. On Linux aeri requests the source rate and reports the rate
   actually negotiated, so the interface can show whether the path is bit-perfect.
3. **Gapless holds across transitions that keep sample rate and channel layout.** A
   format change re-initialises the output; that gap is accepted.

## Consequences

- Two backends to write and maintain, with FFI in `aeri-audio`. Device enumeration and
  hot-plug handling are ours.
- Linux CI runners have no audio server. Backends are verified by hand and by unit
  tests of the callback body, not in CI.
- If PipeWire declines the source rate it resamples in the server, so aeri's own
  resampler is involved only when a macOS device refuses a rate.
- fooyin supports five Linux outputs; aeri supports one. A user without PipeWire
  cannot use aeri. Revisit only if a supported distribution ships without it.
