# ADR 0016 — Device sinks feed the callback through rtrb and position counters

- **Status:** Accepted
- **Date:** 2026-11-16

## Context

Step 4 of the Phase 2–3 brief adds the first device sink and the first code that runs on a real-time audio thread. The engine pushes interleaved blocks through `OutputSink`; the platform pulls samples from a callback that must not allocate, lock, block, or panic. Between them sits a single-producer, single-consumer hand-off that also carries pause, flush (seek, stop, skip), and the played-frame count.

The brief names `rtrb` for the ring buffer and `coreaudio-rs` for macOS. When checked on 2026-11-16, `coreaudio-rs` 0.14.2 had moved from `coreaudio-sys` (bindgen at build time) to the `objc2-*` bindings, and exposes a safe `AudioUnit` that is `Send` and hands the render callback a `&mut [f32]`. Its trampoline allocates nothing per call. It holds one `assert!` on the buffer size CoreAudio passes, so a CoreAudio contract violation would panic on the audio thread. Writing our own trampoline over `objc2-audio-toolbox` was the alternative; it would roughly triple the `unsafe` code for no behaviour aeri needs.

A flush has to discard audio that is already in the ring. Only the consumer can remove items from an `rtrb` ring, so the callback must do the discarding, and it must never discard audio the engine wrote *after* the flush.

## Decision

- **Crates.** `rtrb` 0.4 for the ring, and on macOS `coreaudio-rs` 0.14 with the `audio_unit` and `core_audio` features. Neither type appears in `aeri-audio`'s public API.
- **The bridge** is platform-independent code in `aeri-audio`: a producer half owned by the sink and a renderer half owned by the callback. The callback's inputs are exactly the ring's consumer and these atomics, all `Relaxed`, each an independent fact:
  - `paused` (`AtomicBool`): the callback writes silence and consumes nothing.
  - `flush_at` (`AtomicU64`): written by the producer only. The position, in samples since the stream was opened, before which nothing may be played.
  - `read_pos` (`AtomicU64`): written by the callback only. The position of the next sample it will consume, counting discarded samples as consumed.
  - `underruns` (`AtomicU64`): the callback counts buffers it could not fill; the sink logs the count from its own thread.
- **Flush without a handshake.** `flush` stores the producer's write position into `flush_at` and returns; the engine may write new audio at once. Each callback reads the ring's fill level *first* and `flush_at` *second*, then discards up to `flush_at` before playing. Because `rtrb` publishes new samples with release/acquire, a callback that can see post-flush audio is guaranteed to see the flush that preceded it. So no device buffer ever mixes pre-flush and post-flush audio: at most one buffer of pre-flush audio may finish playing after `flush` returns, and it is followed by post-flush audio only from the next buffer on.
- **Derived values.** `frames_played` is `read_pos − flush_at` (floored at zero) in frames, so a flush resets it with no extra state. `is_drained` is `read_pos ≥` the producer's write position. A `configure` builds a fresh ring and fresh counters.
- **Sizing.** The ring holds 200 ms at the configured spec.
- **`unsafe`.** None is needed for the sink, because `coreaudio-rs` provides the safe surface. `unsafe` in `aeri-audio` is opted into item by item with `#[expect(unsafe_code, reason = …)]`, not crate-wide. That keeps the expectation fulfilled on Linux, where the macOS code does not compile.

## Consequences

- Every atomic is `Relaxed` and owned by one writer. RUST_STYLE §5's rule holds without exception.
- Seek latency is up to one device buffer (typically 5–12 ms), during which stale audio may finish playing. That is inaudible next to the seek itself.
- 200 ms of buffering means pause and volume (Phase 4) act on audio already in the ring; pause takes effect at once because it stops consumption rather than draining.
- `coreaudio-rs`'s `set_device_sample_rate` matches only devices that list the exact rate as a discrete value, and waits up to two seconds on a channel. It runs in `configure`, never on the audio thread.
- `objc2`'s build script (target-ABI detection only, read on 2026-11-16) is named in `deny.toml`.
- Device loss (unplugging during playback) is not detected yet; `SinkError::DeviceLost` has no producer on macOS. Revisit with hot-plug handling.
- Revisit if a platform API needs the callback to pull a different number of frames than it is given, or if `rtrb` changes its publication ordering.
