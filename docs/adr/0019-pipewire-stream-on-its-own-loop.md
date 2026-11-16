# ADR 0019 — On Linux, a PipeWire stream runs on its own loop thread and requests the source rate

- **Status:** Accepted
- **Date:** 2026-11-16

## Context

ADR 0003 gives Linux one output, `PipeWireSink`, which asks for the source rate and reports the rate actually used. The brief names the `pipewire` crate. `pipewire` 0.10.1 (MIT, published 2026-08-19) was read on 2026-11-16 and probed against PipeWire 1.6.11 on the development machine:

- Its main loop, context, core, and stream are not `Send`, but `OutputSink` is. Whatever owns them has to stay on one thread.
- `ThreadLoop` would run the loop on a thread PipeWire creates, which gets around the workspace rule that threads go through `thread::Builder` with a name. Its API also expects callers to take its lock around every call.
- With `StreamFlags::RT_PROCESS` the `process` callback runs on PipeWire's real-time data thread. The crate's trampoline calls the closure directly. `dequeue_buffer` and the buffer's drop call `pw_stream_dequeue_buffer` and `pw_stream_queue_buffer`, which allocate nothing. The trampoline has two `usize::try_from(u32).unwrap()` calls, which cannot fail on aeri's 64-bit targets.
- The buffer comes back as `&mut [u8]` holding the whole mapped buffer: 98,304 bytes in the probe. `Buffer::requested()` (feature `v0_3_49`) gave the frames PipeWire wanted that cycle, 471–498 for a 44.1 kHz stream on a 48 kHz graph. The buffers were 16-byte aligned.
- Stream states came in the order `Connecting`, `Paused` (0.8 ms), then `Streaming` (5 ms). The format is negotiated between `Paused` and `Streaming`. Once streaming, `Stream::time().rate` gave the graph rate.
- The server owns the graph rate. A stream's `node.rate` is a request, and the graph switches only to a rate listed in `clock.allowed-rates`. On the development machine that list is `[ 48000 ]`, so a 44.1 kHz stream stayed at 48 kHz and PipeWire resampled.
- `pipewire-sys` and `libspa-sys` run `bindgen` at build time against the system's PipeWire headers. That needs libclang and the headers on every Linux build, and it adds build-time crates under three licences not on the allowlist: `bindgen` (BSD-3-Clause), `libloading` (ISC), and `target-lexicon` (Apache-2.0 WITH LLVM-exception). None of them is linked into aeri.

## Decision

- **Crate.** `pipewire` 0.10 with only `v0_3_49`, as a Linux-only dependency of `aeri-audio`. None of its types appear in the public API.
- **A thread per stream.** Each `configure` starts a thread named `aeri-pipewire`. It owns the main loop, context, core, stream, and listener, and runs the loop until it is told to stop. The sink keeps only:
  - the bridge's `Writer` (ADR 0016);
  - a `pipewire::channel::Sender` whose message makes the loop quit;
  - a `std::sync::mpsc` receiver on which the thread reports failures;
  - the thread's `JoinHandle`.

  Dropping the stream sends the stop, disconnects the stream on its own thread, and joins it.
- **The stream.**
  - Properties: `media.type` Audio, `media.category` Playback, `media.role` Music, and `node.rate` set to `1/<source rate>`.
  - Format: one `EnumFormat`, interleaved `F32LE` at the source rate and channel count. Positions are `MONO` for one channel and `FL FR` for two; more channels are left unpositioned.
  - Flags: `AUTOCONNECT | MAP_BUFFERS | RT_PROCESS`. Never `EXCLUSIVE`, `NO_CONVERT`, or `node.force-rate`, for the same reason as ADR 0017: other applications keep their audio.
- **`configure`** waits up to two seconds for `Streaming`, then returns the requested spec as negotiated, because PipeWire converts whatever the graph runs at.
  - It returns `NoDevice` if the server cannot be reached or streaming never starts.
  - It returns `FormatRefused` if the stream reports an error first.
- **The rate report.** On `Streaming`, the loop thread reads the graph rate from `Stream::time()`. If it differs from the stream's rate, it logs at `info` that PipeWire resamples; if the two match, at `debug`. `OutputSink` is not changed. The roadmap's "report the negotiated rate" is met by this log until a frontend needs the value, and that will be a trait change made with the owner.
- **The callback.** It dequeues a buffer and renders `requested` frames, or all that fit if `requested` is 0, with one `Renderer::render` call over the buffer viewed as `&mut [f32]`. Then it sets the chunk's offset, stride, and size. One render call per buffer keeps ADR 0016's rule that no device buffer mixes pre-flush and post-flush audio. A buffer that is not aligned for `f32` is returned empty, so PipeWire plays silence.
- **`unsafe`.** The byte-to-sample view is `align_to_mut::<f32>`, in one function that opts in with `#[expect(unsafe_code)]`. Any bit pattern is a valid `f32`, and the bytes are already exposed as initialised `&mut [u8]`.
- **Device loss.** An error state after streaming has started is sent on the failure channel and quits the loop. From then on `write` returns `SinkError::DeviceLost`. This is that variant's first producer.
- **The build.**
  - `deny.toml` gets licence exceptions for those three crates only. The build scripts `cargo-deny` reports for the new crates are added to `allow-build-scripts`.
  - The CI setup action installs `libpipewire-0.3-dev` and `libclang-dev` on Linux runners.

## Consequences

- **Bit-perfect is conditional on Linux.** It needs three things to hold:
  - the graph runs at the source rate, which takes the rate in `clock.allowed-rates` and an idle graph when aeri starts;
  - nothing else is playing;
  - the volume is at unity.

  aeri reports the first and cannot detect the other two. On the development machine the first never holds.
- Every Linux build needs PipeWire headers and libclang, and building the bindings is slow. Packagers inherit the same requirement. A user without PipeWire cannot run aeri (ADR 0003).
- A format change between tracks costs a new connection. The probe reached `Streaming` in 5 ms. This is the re-initialisation gap ADR 0003 already accepts.
- `frames_played` counts frames handed to PipeWire, which holds about one quantum (10–20 ms) more before the device. Position runs that much ahead, as it does with CoreAudio's device buffer.
- Tearing the stream down as soon as `is_drained` is true may cut the last quantum. This has not been checked by listening; revisit with `Stream::flush(true)` and the `drained` event if it is audible.
- A panic in the callback aborts the process instead of unwinding into C, because Rust aborts on unwinding out of an `extern "C"` function.
- **Not verified:**
  - the graph switching to the source rate where `clock.allowed-rates` allows it, because the development machine allows only 48 kHz;
  - channel mapping for more than two channels;
  - `DeviceLost` when the PipeWire server restarts during playback.
- Revisit if `pipewire` gains typed sample access to buffers, which would remove the `unsafe`, or when multichannel layouts are needed.
