# ADR 0004 — Native output backends; bit-perfect and gapless claims scoped

- **Status:** Accepted
- **Date:** 2026-11-14

## Context

The stack listed three output layers: `cpal`, `pipewire`, and `coreaudio-rs`. On Linux
cpal reached the device through ALSA (CI installed `libasound2-dev` for it), against
the "no raw ALSA" rule. On macOS the features the thesis rests on — hog mode and
device sample-rate switching — are not exposed by cpal and need CoreAudio directly.
cpal was therefore an abstraction over exactly the parts aeri has to reach beneath.

Two locked decisions also overclaimed:

- "Bit-perfect: device must match source" assumed the app controls the device rate.
  Under PipeWire the server owns the graph rate. A stream can request a rate; whether
  the graph switches is server policy (allowed rates, whether the graph is idle).
- "Gapless: always on" cannot hold across a device re-init, which a sample-rate change
  between tracks requires.

## Decision

1. **No cpal.** `OutputSink` has two native implementations in `aeri-audio`:
   `CoreAudioSink` (`coreaudio-rs`) and `PipeWireSink` (`pipewire`), selected by
   `cfg(target_os)`. Tests use `FakeSink`.
2. **Bit-perfect is guaranteed on macOS only**, in hog mode with the device switched to
   the source rate. On Linux it is best-effort: aeri requests the source rate and
   reports the rate actually negotiated, so a frontend can show whether the path is
   bit-perfect. It is never claimed when the negotiated rate differs.
3. **Gapless holds across transitions that keep sample rate and channel layout.** A
   format change re-initialises the output; that gap is accepted and not papered over.

## Consequences

- Two backends to write and maintain instead of one, and more FFI in `aeri-audio`.
  Device enumeration and hot-plug handling are ours.
- `libasound2-dev` is no longer a build dependency and is removed from CI.
- Linux CI runners have no audio server; backend behaviour is verified by hand and by
  `FakeSink`-level tests, not in CI.
- If PipeWire declines the source rate it resamples in the server, so aeri's own
  resampler stays out of that path too (see "On the resampler being bypassed").
