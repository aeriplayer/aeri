# Design brief — Phases 2 and 3: first audio

This is the specification for taking aeri from empty crates to `aeri play file.flac` producing sound. It is written to be executed step by step. Where it gives a type or trait, build that shape; where code and brief disagree after you have tried it, stop and raise it (see "When to stop and ask") rather than quietly diverging.

Read first: [`ARCHITECTURE.md`](../ARCHITECTURE.md), [`RUST_STYLE.md`](../RUST_STYLE.md),
[`TESTING.md`](../TESTING.md),
ADRs [0003](../adr/0003-native-output-backends.md),
[0004](../adr/0004-engine-is-an-actor.md),
[0005](../adr/0005-engine-plays-current-and-next.md),
[0006](../adr/0006-frontends-use-the-player-handle.md),
[0007](../adr/0007-symphonia-flac-mp3-first.md),
[0009](../adr/0009-open-tag-metadata.md).

## Outcome

```bash
cargo run -p aeri-cli -- play crates/aeri-decode/tests/fixtures/sine.flac
```

plays the file through the default output device, prints position as it goes, stops cleanly on Ctrl-C or end of track, and exits 0. Every step below lands with its tests, and `prek run --all-files` plus `prek run --all-files --stage pre-push` stay green.

## Rules for this work

- **Signatures here are shapes, not gospel.** The `OutputSink` and `Decoder` traits and the core types other than `PlaybackState` were compile-checked under the workspace lint set on Rust 1.100. `PlaybackState`, the `Engine` API, and every method body were not. If the compiler or clippy objects, fix the signature in the idiomatic direction and keep the intent.
- **Verify every external API before using it.** symphonia 0.6, rtrb, coreaudio-rs, pipewire, and tokio all move. Read the docs for the version `cargo add` resolves (`cargo doc --open -p <crate>` or docs.rs); do not write calls from memory.
- **Add each dependency with the first line of code that uses it** (`cargo add`, then move the version into `[workspace.dependencies]`). `cargo-shear` rejects unused ones; `cargo-deny` rejects new licences and new build scripts until `deny.toml` is updated deliberately; `cargo xtask check-deps` rejects default features left on.
- **One step per commit**, on a `phase-3` branch.

## Step 1 — `aeri-core`: playback types

No dependencies on other workspace crates. No `serde` yet (nothing is persisted until the library phase). Everything derives `Debug`; value types also derive `Clone, PartialEq, Eq, Hash` where that is truthful.

```rust
/// Sample rate in Hz. Never zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SampleRate(NonZeroU32);

/// Number of interleaved channels. Never zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Channels(NonZeroU16);

/// The shape of a PCM stream. Two streams are gapless-compatible iff their specs are equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StreamSpec {
    pub sample_rate: SampleRate,
    pub channels: Channels,
}

/// Where a track's audio comes from. `#[non_exhaustive]`: streams arrive later.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Source {
    File(PathBuf),
}

/// The minimal track (ADR 0009). The full metadata type arrives with the library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub source: Source,
    pub duration: Option<Duration>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Buffering,
    Playing,
    Paused,
}

/// What the player is doing. A position cannot exist without a track: the type makes
/// "stopped at 1:23" unrepresentable instead of documenting that it must not happen.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlaybackState {
    Stopped,
    Active {
        track: Arc<Track>,
        transport: Transport,
        position: Duration,
        /// The spec the output actually negotiated (ADR 0003). `None` until known.
        output: Option<StreamSpec>,
    },
}
```

There is no queue or playlist type here. The engine knows a current track and a next slot (ADR 0005); playlists arrive with `aeri-player` in Phase 4.

Idiom notes:

- **Newtypes with private fields** (`SampleRate`, `Channels`) put validation in one constructor (`SampleRate::new(u32) -> Option<Self>`, plus `TryFrom<u32>`), so no other code re-checks for zero. Provide `get()` accessors.
- **`Duration` for time**, not `f64` seconds or raw frame counts, at every API boundary.
- **`Arc<Track>`** because a track is shared between the engine, the state snapshot, and subscribers; cloning the `Arc` is a pointer copy.
- **`#[non_exhaustive]`** on enums that will grow, so adding a variant later is not a breaking change for downstream `match`es.

Done when:

- unit tests show the `SampleRate` and `Channels` constructors reject zero and round-trip every other value through `get()` and `TryFrom`;
- `prek run mutants --stage manual --all-files` leaves no surviving mutant in `aeri-core`;
- the `coverage` hook in `.pre-commit-config.yaml` gains a third line, `&& cargo llvm-cov report --package aeri-core --fail-under-lines 100`, and passes ([`TESTING.md`](../TESTING.md));
- `--no-tests=pass` is removed from the `cargo-nextest` and `coverage` hooks in `.pre-commit-config.yaml`. It exists only because the crates are empty; from this step on, a run that finds no tests is a failure.

## Step 2 — `aeri-audio`: `OutputSink` and `FakeSink`

Depends on `aeri-core`. The trait is push-style and block-oriented: the engine pushes interleaved `f32` blocks; a device sink hides its ring buffer and real-time callback behind it. That keeps `rtrb` out of the public API and lets a test sink be a `Vec`.

```rust
pub trait OutputSink: Send {
    /// Opens (or re-opens) the output for `requested` and returns the spec actually
    /// negotiated. The two differ when the device or server declines (ADR 0003).
    fn configure(&mut self, requested: StreamSpec) -> Result<StreamSpec, SinkError>;

    /// Offers interleaved samples. Never blocks. Returns how many whole *frames* were
    /// accepted, which may be zero when the buffer is full.
    fn write(&mut self, interleaved: &[f32]) -> Result<usize, SinkError>;

    /// Stops or resumes consumption without discarding buffered audio.
    fn set_paused(&mut self, paused: bool);

    /// Discards everything buffered but not yet played (seek, stop, track skip).
    fn flush(&mut self);

    /// Frames handed to the device since the last `configure` or `flush`.
    /// Position reporting derives from this, not from how much was decoded.
    fn frames_played(&self) -> u64;

    /// True once every accepted frame has been played.
    fn is_drained(&self) -> bool;
}
```

`SinkError` is a `thiserror` enum, `#[non_exhaustive]`, with variants for the failures a caller can act on differently (no device, format refused, device lost) and the platform error kept as `#[source]`.

`FakeSink` implements the trait by recording into memory, with knobs for tests: a capacity (to exercise back-pressure), a spec to "negotiate" (to exercise the mismatch path), and a log of calls (`configure`, `flush`) so tests can assert on re-inits. It lives in `aeri-audio` behind a `test-util` cargo feature so `aeri-engine`'s tests can enable it as a dev-dependency without shipping it.

Done when: `FakeSink` has tests for back-pressure (partial acceptance, whole frames only), pause, flush, and `frames_played`.

## Step 3 — `aeri-decode`: `Decoder` over symphonia

Depends on `aeri-core`. symphonia stays a private implementation detail: no symphonia type appears in a public signature.

```rust
pub trait Decoder: Send {
    fn spec(&self) -> StreamSpec;
    fn duration(&self) -> Option<Duration>;

    /// Decodes the next block and lends it out as interleaved `f32`.
    /// `Ok(None)` is a clean end of stream. The slice borrows the decoder's own buffer,
    /// so steady-state decoding allocates nothing.
    fn next_block(&mut self) -> Result<Option<&[f32]>, DecodeError>;

    /// Seeks to the nearest decodable point at or before `to`; returns where it landed.
    fn seek(&mut self, to: Duration) -> Result<Duration, DecodeError>;
}

/// Probes `source` and returns a decoder for it.
pub fn open(source: &Source) -> Result<Box<dyn Decoder>, DecodeError>;
```

`DecodeError` distinguishes at least: I/O failure, unsupported or unrecognised format, and corrupt data — and corrupt *packets* mid-stream are skipped with a `tracing::warn!`, not surfaced as fatal, because one bad frame should not end a track.

Formats: FLAC and MP3, and only those symphonia features (ADR 0007).

Fixtures: commit a one-second 440 Hz stereo 44.1 kHz `sine.flac` and `sine.mp3` under `crates/aeri-decode/tests/fixtures/` (each far below the 256 KiB hook limit), plus the command that generated them in a `README.md` beside them.

Done when:

- integration tests decode each fixture and assert the spec, a total frame count within codec padding of 44 100, non-silent output, and a seek to 0.5 s that lands at or before it;
- a `cargo-fuzz` target feeds arbitrary bytes to `open` + `next_block` and has run for at least ten minutes locally with no panic, hang, or unbounded allocation. Commit the target under `fuzz/` and any crashing inputs it finds as regression fixtures. This needs a nightly toolchain and a CI job (short run per pull request); both are new tooling, so raise it with the user when you reach this step.

## Step 4 — `aeri-audio`: a real device sink

Write the sink for the platform you are developing on first (`CoreAudioSink` on macOS, over `coreaudio-rs`), behind `cfg(target_os)`. The other platform's sink is its own step and its own commit. The bridge between the sink and the callback is specified in [ADR 0016](../adr/0016-real-time-bridge.md); the macOS rate policy, with no hog mode, in [ADR 0017](../adr/0017-bit-perfect-without-hog-mode.md).

`coreaudio-rs` provides a safe `AudioUnit`, so the sink itself needs no `unsafe`. Where `unsafe` is needed in `aeri-audio`, opt in on the item with `#[expect(unsafe_code, reason = "…")]`.

The real-time callback is the most constrained code in aeri. Inside it:

- **No allocation, no deallocation.** No `Vec::push`, `Box`, `String`, `format!`, `Arc::clone` whose drop could free, or `tracing` macros.
- **No locks.** No `Mutex`, `RwLock`, channel send/recv, or `OnceLock` initialisation.
- **No syscalls** beyond what the audio API itself performs. No I/O, no `thread::park`.
- **No panics.** No indexing that can go out of bounds, no `unwrap`/`expect`, no arithmetic that can overflow in debug. A panic across FFI is undefined behaviour.
- **Inputs are exactly:** the `rtrb` consumer, and the `Relaxed` atomics of ADR 0016 (`paused`, `flush_at`, `read_pos`, `underruns`). Underrun writes silence and bumps `underruns`, which the sink logs from its own thread.

Test the callback body as a plain function over a slice, off the audio thread, inside the in-house allocation guard of [ADR 0018](../adr/0018-real-time-verification.md), so an accidental allocation fails a test, not a listening session.

Done when:

- a manual run plays a sine through the speakers with no audible glitches;
- the callback function has unit tests for normal fill, underrun, pause, and flush, run under the allocation guard;
- those tests pass under Miri (`prek run miri --stage manual --all-files`); tests that call the platform API are marked `#[cfg_attr(miri, ignore)]` with the reason;
- the `loom` model of ADR 0018 shows that no interleaving plays a pre-flush sample after a post-flush one, plays a post-flush sample twice or out of order, or loses frames across a pause. It runs from a manual-stage `loom` hook, and in CI on pull requests that change `aeri-audio`.

## Step 5 — `aeri-engine`: the playback actor

Depends on `aeri-core`, `aeri-audio`, `aeri-decode`. The engine is an **actor** (ADR 0004): one thread owns all playback state; everything else talks to it through a handle. There is no `Mutex<EngineState>` anywhere. It knows two tracks, the current one and an optional next (ADR 0005), and nothing about playlists.

```rust
/// A cheap, cloneable handle to the playback thread.
#[derive(Debug, Clone)]
pub struct Engine { /* command sender + state receiver */ }

impl Engine {
    /// Starts the playback thread, which takes ownership of `sink`.
    pub fn spawn(sink: Box<dyn OutputSink>) -> Result<Self, EngineError>;

    /// Starts the playback thread on this platform's default output device.
    pub fn with_default_output() -> Result<Self, EngineError>;

    /// Plays `track` now, replacing the current track and clearing the next slot.
    pub async fn play(&self, track: Arc<Track>) -> Result<(), EngineError>;
    /// Fills or clears the slot the engine takes when the current track ends.
    pub async fn set_next(&self, track: Option<Arc<Track>>) -> Result<(), EngineError>;
    pub async fn pause(&self) -> Result<(), EngineError>;
    pub async fn resume(&self) -> Result<(), EngineError>;
    pub async fn stop(&self) -> Result<(), EngineError>;
    pub async fn seek(&self, to: Duration) -> Result<(), EngineError>;

    /// Latest-value subscription. A new subscriber sees the current state at once.
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<PlaybackState>;
}
```

Inside:

- **Commands** travel on a `tokio::sync::mpsc` channel as a private `enum Command`, each variant carrying a `tokio::sync::oneshot::Sender` for its reply. The async methods above send and await the reply. These tokio primitives do not need a runtime on the receiving side: the playback thread is a plain named `std::thread` and uses `blocking_recv` when idle and `try_recv` between blocks while playing.
- **State** is published through a `tokio::sync::watch` channel. Only the playback thread writes it. Publish on every transition, and position at most ~10×/second.
- **The loop** while playing: drain pending commands → get a block from the decoder → `sink.write` until accepted, sleeping a few milliseconds when the sink is full (do not build a wake-up from the real-time callback) → update position from `sink.frames_played()`.
- **Gapless:** when the current decoder returns `Ok(None)`, take the next slot. Empty → drain and stop. Equal `StreamSpec` → keep writing into the same sink with no `configure` and no flush. Different spec → wait for `is_drained`, then `configure`. This is ADR 0003's rule and the tests below pin it.
- **No "took the next track" event yet.** ADR 0005's event has no consumer until `aeri-player` exists, so it is built in Phase 4. Here the change is visible only as a new `track` in the published state.
- **Shutdown:** when the last `Engine` handle drops, the command channel closes; the thread sees that, stops the sink, and exits. No explicit "shutdown" call that a caller could forget.
- **Errors after a command was accepted** (a file that fails mid-decode) are logged with `tracing::error!`, the track is skipped, and the state moves on. The engine never panics on bad input.

`Box<dyn OutputSink>` rather than `Engine<S: OutputSink>`: the sink is called once per block, so dynamic dispatch costs nothing measurable, and a non-generic `Engine` keeps everything above it non-generic too.

`with_default_output` exists so that a frontend never names `aeri-audio`: only the engine may depend on it (`deny.toml`).

Done when these integration tests pass against `FakeSink`:

1. Play one fixture → the sink received exactly the decoded frame count, not silence; observed states go `Buffering → Playing → Stopped`.
2. Pause → `frames_played` stops advancing; resume → it continues; no frames lost.
3. Seek to 0.5 s → sink was flushed once; total frames ≈ half the track.
4. `play` then `set_next` with the same spec → exactly one `configure`, frames = sum of both, and the state's `track` changes without passing through `Stopped`.
5. `play` then `set_next` with a different spec → two `configure` calls, in order.
6. A missing file → `play` returns `Err`, state stays `Stopped`.
7. Negotiated spec differs from requested → `PlaybackState::Active.output` reports the negotiated one.
8. Dropping every `Engine` handle joins the thread (the test would hang otherwise; wrap it in a timeout).
9. `set_next(None)` after `set_next(Some(…))` → playback stops at the end of the current track.

## Step 6 — `aeri-cli`: `aeri play <path>`

- Argument parsing with `clap`'s derive API; a `Command` enum with one variant, `Play`.
- `main` returns `anyhow::Result<()>`; a small `#[tokio::main(flavor = "current_thread")]` runtime is enough.
- Depend on `aeri-engine` and `aeri-core` only, and build the engine with `Engine::with_default_output()`. Depending on the engine directly is a temporary exception that ends when `aeri-player` is created in Phase 4 (ADR 0006).
- Print progress from `subscribe()`; `tokio::select!` it against `tokio::signal::ctrl_c()`. Exit when the state returns to `Stopped`.
- User-facing output goes through one module that carries `#![expect(clippy::print_stdout, reason = "CLI output is the product")]`. Diagnostics go through `tracing`, initialised to stderr.

Done when: the outcome at the top of this document works by hand, and an integration test runs the binary against a missing path and asserts a non-zero exit and a readable error.

## When to stop and ask

Stop and raise it with the user, do not improvise, when:

- a signature here cannot be made to compile without changing its *meaning*;
- you want a dependency this brief does not name;
- you need `unsafe` outside `aeri-audio`, or a lock or allocation in the callback;
- a decision recorded in an ADR looks wrong once real code exists;
- a check fails and the only fix you can see is to weaken the check.

## Not in these phases

DSP of any kind (including volume and ReplayGain), resampling, playlists, the queue, playback order, the `Player` handle, the library, scripting, MPRIS, and the GUI. If a step seems to need one of these, the step is being over-built.
