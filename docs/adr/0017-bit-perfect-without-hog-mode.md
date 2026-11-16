# ADR 0017 — On macOS, aeri switches the device rate but never takes it exclusively

- **Status:** Accepted
- **Date:** 2026-11-16
- **Supersedes:** decision 2 of [ADR 0003](0003-native-output-backends.md), and its consequence about aeri's own resampler on macOS

## Context

ADR 0003 promised bit-perfect output on macOS in hog mode with the device switched to the source rate. Hog mode gives one process exclusive use of the device: while aeri held it, every other app (calls, notifications, video) would be silent. That is the wrong default for a desktop player used alongside everything else, and the owner does not want it.

Without hog mode, two things still affect whether samples reach the device unchanged: the device's nominal rate, and CoreAudio's mixer. Switching the device to the source rate removes resampling. The mixer is believed, from memory and unverified, to pass a single source through unchanged at unity system volume; any other app's audio, or a volume below maximum, alters the samples.

## Decision

- `CoreAudioSink::configure` asks the default output device for the source sample rate. If the device does not switch, it stays at its current rate and CoreAudio's output unit converts.
- aeri reads the device's offered rates itself to say why. A rate the device does not offer is routine (Bluetooth headphones offer 24 and 48 kHz only), so it is logged at `info`, naming the device and the rates it offers. A rate the device offers but would not switch to is logged at `warn`, because that is a fault worth seeing.
- The sink accepts the requested spec in either case and reports it as negotiated, because it can always consume it.
- aeri never enables hog mode. If it is ever offered, it is an opt-in setting with its own ADR, after settings storage is decided.

## Consequences

- Other apps keep sounding while aeri plays. Switching the device rate resamples their audio instead of aeri's.
- **Bit-perfect is conditional on macOS**: it holds when the device took the source rate, nothing else is playing, and system volume is at maximum. aeri cannot detect the last two conditions. This is weaker than ADR 0003's promise.
- The mixer claim above is unverified. Check it by capturing the device output (for example through a loopback device) and comparing it with the decoded file, before any user-facing text says "bit-perfect".
- aeri's own resampler is not needed on macOS: CoreAudio converts when the device refuses a rate. Phase 4's resampler remains for Linux if PipeWire paths need it.
- Changing the device rate is visible system-wide (Audio MIDI Setup shows it) and persists after aeri exits. Restoring the previous rate on exit is not done; revisit if it annoys.
