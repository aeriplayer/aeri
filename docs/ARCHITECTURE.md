# aeri — Architecture

This document is the authoritative reference for system design, structural constraints, and non-obvious decisions. Process, conventions, and build instructions live in `README.md`. Architectural decisions are recorded as ADRs in `docs/adr/`; the rules for when one is required are in [`adr/README.md`](adr/README.md). Rust conventions are in
[`RUST_STYLE.md`](RUST_STYLE.md).

---

## Thesis

aeri is a foobar2000-class music player for macOS and Linux, written in Rust ([ADR 0001](adr/0001-desktop-player-scope.md)). Its model is [fooyin](https://github.com/fooyin/fooyin): playlist-centric playback, a library, one script language for display and queries, and an interface assembled from widgets in a layout the user controls.

The bet is not a new kind of player. It is that this kind of player, designed fresh in Rust for two modern platforms, can be smaller, safer, and easier to change than its predecessors: ownership instead of locks, traits instead of a plugin ABI, and no compatibility burden.

---

## Non-negotiable pillars

These are load-bearing constraints. Violating them makes the architecture incoherent.

1. **Modern platforms only.** Apple Silicon macOS and Linux with PipeWire. No compatibility shims. Windows is never considered.
2. **Composable.** Every extension point is a trait with a registry ([ADR 0002](adr/0002-compiled-in-extension-points.md)). Every frontend reaches the application through one `Player` handle ([ADR 0006](adr/0006-frontends-use-the-player-handle.md)). Each crate is usable and testable without the ones above it.
3. **The real-time audio thread is sacred.** No allocations, no locks, no logging, no blocking on the audio thread. The target is zero underruns, not low latency: a music player can buffer generously, and should.
4. **State has one owner.** Long-lived mutable state belongs to one thread and is reached by messages ([ADR 0004](adr/0004-engine-is-an-actor.md)). Shared data is immutable and reference-counted. `unsafe` exists only in `aeri-audio`.
5. **Configuration is data.** Playlist columns, groupings, filters, and searches are scripts ([ADR 0010](adr/0010-one-script-language.md)). The interface is a serialised layout tree ([ADR 0011](adr/0011-gpui-and-layout-as-data.md)). A feature that can be a script or a widget is not hard-coded.
6. **Defensible dependencies.** Every crate addition is justified. Prefer tokio-rs, dtolnay, rust-lang, and established ecosystem authors. No bleeding-edge bets without written rationale.

---

## Crate graph

```text
aeri-core       domain types; no workspace dependencies
    ↑
aeri-script     title formatting + queries                  planned · scripting
aeri-audio      OutputSink + CoreAudio / PipeWire sinks + the real-time thread
aeri-decode     Decoder over symphonia
aeri-dsp        DspNode + chain + resampler                 planned · playback
    ↑
aeri-engine     audio engine: decode → DSP → sink, current track + next slot
aeri-library    SQLite, scanning, tags, in-memory store     planned · library
    ↑
aeri-player     playlists, queue, order; owns engine and
                library; the `Player` handle                planned · playback
    ↑
aeri-platform   MPRIS / macOS Now Playing                   planned · platform
aeri-gui        binary: GPUI shell, widget registry         planned · GUI
aeri-cli        binary: `aeri`
```

**A crate is created in the phase that first needs it.** A stub crate still costs compile time, lockfile entries, and dependency upkeep, and its boundary hardens before any code has tested it. Five crates exist: `aeri-core`, `aeri-audio`, `aeri-decode`, `aeri-engine`, `aeri-cli`.

The split below the engine was drawn before any audio flowed and is provisional: expect the first-audio phase to reshape it, and do not contort code to preserve it.

### Dependency rules

- `aeri-core` has **no** dependencies on other workspace crates.
- `aeri-script` depends only on `aeri-core`. It does no I/O.
- Only `aeri-engine` depends on `aeri-audio`, `aeri-decode`, and `aeri-dsp`.
- Only `aeri-player` depends on `aeri-engine` and `aeri-library`.
- Frontends (`aeri-gui`, `aeri-cli`, `aeri-platform`) depend on `aeri-player`, `aeri-script`, and `aeri-core`, and on nothing beneath.
- **Temporary exception:** until `aeri-player` exists, `aeri-cli` depends on `aeri-engine` directly (ADR 0006).
- No cyclic dependencies. If you reach for one, the abstraction boundary is wrong.

`deny.toml` enforces these mechanically.

---

## How the pieces relate

| fooyin | aeri | Notes |
| --- | --- | --- |
| Audio engine and pipeline | `aeri-engine` | Decode, DSP, output; knows two tracks |
| Player controller, playlist handler, queue | `aeri-player` | Decides what plays next |
| Music library, database | `aeri-library` | Persistence and the in-memory store |
| FooScript | `aeri-script` | Formatting and queries |
| Input, output, DSP plugins | `Decoder`, `OutputSink`, `DspNode` | Compiled in (ADR 0002) |
| GUI widgets and layout provider | `aeri-gui` widget registry and layout tree | |
| MPRIS and media-control plugins | `aeri-platform` | A frontend (ADR 0006) |

The table is a map for reading fooyin, not a promise of matching structure.

---

## Threading model

```text
┌──────────────────────────────────────────────┐
│  Frontend executor (GPUI, or the CLI's)      │
│  awaits `Player`; renders its state          │
└────────────────┬─────────────────────────────┘
                 │ commands (mpsc + oneshot reply) · state (watch) · events
                 ▼
┌──────────────────────────────────────────────┐
│  Player thread                               │
│  playlists · queue · order · chooses next    │
└───────┬──────────────────────────┬───────────┘
        │ play, set_next, seek     │ scan, edit, snapshot
        ▼                          ▼
┌──────────────────────┐   ┌──────────────────────┐
│  Engine thread       │   │  Library threads     │
│  decode → DSP →      │   │  SQLite writer ·     │
│  sink.write          │   │  scanner (rayon)     │
└───────┬──────────────┘   └──────────────────────┘
        │ rtrb SPSC ring buffer (samples) + atomics
        ▼
┌──────────────────────────────────────────────┐
│  Audio output thread (real-time priority)    │
│  NO allocation · NO locks · NO logging       │
│  reads the ring buffer and atomics only      │
└──────────────────────────────────────────────┘
```

Each box that owns state is an actor (ADR 0004): one thread, a command channel in, a watch channel out. No lock guards any of it.

**There is no application-wide async runtime.** The actors are plain named threads. Their handles expose `async` methods that complete when the actor replies, and any executor can await them. `tokio::sync` channels are used as runtime-independent primitives; a tokio runtime exists only where a binary chooses to start one.

**The audio thread never interprets a message.**

- Commands go to the engine thread. Seek, track change, and stop change what is written into the ring buffer. Volume is a DSP node, so it lives there too.
- The audio callback's only inputs are the `rtrb` ring buffer and a handful of atomics (paused, flush generation, frames played).
- A seek bumps the flush generation; the callback discards stale samples until the generation it reads matches.

The design target is **zero underruns**. Latency between a command and its audible effect is bounded by the buffered audio and is not a goal to minimise.

`unsafe` is permitted only in `aeri-audio`, for FFI and the real-time bridge. Every `unsafe` block carries a `// SAFETY:` comment.

---

## Playback

### Engine: current and next

The engine plays one track and holds an optional next ([ADR 0005](adr/0005-engine-plays-current-and-next.md)). When the current track ends it takes the slot, continues gaplessly if the stream spec is unchanged, and emits an event. It has no notion of playlist or order.

### Player: what plays next

`aeri-player` owns everything that decides the next track:

- **Playlists** are ordered lists of tracks, persisted by the library. One is active.
- **Playback order** (default, repeat track, repeat playlist, shuffle) picks the next entry of the active playlist.
- **The queue** overrides the order: queued tracks play first, then the order resumes.
- **Stop after current** clears the slot.

On each engine event, and on any edit that changes the answer, the player recomputes the next track and calls `set_next`. Entries are named by stable id, never by index.

### Pipeline

Linear, not a graph. The constraint is load-bearing: it keeps latency predictable, simplifies gapless pre-roll, and removes a class of topology bugs.

```text
Decoder → convert to f32 → DSP chain → resampler → OutputSink
                               ↑
                    [ReplayGain, EQ, volume, …]
```

A DSP chain is an ordered list of `DspNode`s named by registry id (ADR 0002), so it can be saved, reordered, and edited as data.

### Locked decisions

Not up for reconsideration without an ADR.

| Decision | Value | Rationale |
| --- | --- | --- |
| Internal sample format | `f32` | Sufficient precision; avoids per-node format negotiation. Interleaved at the `Decoder` and `OutputSink` boundaries; layout inside the DSP chain is decided with `aeri-dsp` |
| Block size | 1024 frames (internal) | The DSP block size, **not** the device period. The callback buffer size is chosen by the host audio API and must not be forced |
| Sample rate | Follow source; re-init output on change | Bit-perfect on macOS in hog mode; best-effort on Linux, where the negotiated rate is reported, never assumed ([ADR 0003](adr/0003-native-output-backends.md)) |
| Gapless | Always on, across transitions that keep rate and channel layout | A format change re-inits the output and that gap is accepted (ADR 0003) |
| ReplayGain | On by default, track and album modes | |
| Volume | Software, in the DSP chain | Hardware volume off by default, for bit-perfect output |
| Resampler | `rubato` | Bypassed on the normal path; see below |

#### On the resampler being bypassed

"Follow source" means the output rate normally equals the source rate, so the resampler does no work on the common path. It exists for one case: a macOS device refuses the source rate, and aeri falls back to the nearest supported rate. On Linux, PipeWire resamples in the server when it declines a rate. Without this note the terminal node reads as dead code.

#### On fades and crossfade

fooyin offers configurable fades on pause, stop, and seek, and crossfade on track change. Crossfade and gapless are per-transition behaviours, not rivals. Crossfade needs a second stream decoding at once and a mixer; the next slot (ADR 0005) is where that stream comes from. Both are scheduled after the first GUI release, as scope, not because the design excludes them.

### Formats

FLAC and MP3 ([ADR 0007](adr/0007-symphonia-flac-mp3-first.md)). Further formats are added on demand behind the `Decoder` trait. No C codec, no DRM-protected files.

---

## Library

[ADR 0008](adr/0008-library-sqlite-and-in-memory-store.md).

- **Persistence** is SQLite in WAL mode, on `rusqlite` with bundled SQLite. One connection on one dedicated thread owns every write.
- **Queries run in memory.** The library holds every track in an immutable snapshot shared by `Arc`. A scan or tag edit publishes a new snapshot on a watch channel. Filters, sorts, groupings, and searches are scripts evaluated over the snapshot.
- **Metadata** is an open, multi-value tag map with typed technical properties ([ADR 0009](adr/0009-open-tag-metadata.md)). Its full shape and the SQLite schema are designed together at the start of the library phase.

**Required PRAGMAs**, applied when the connection opens:

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous  = NORMAL;
PRAGMA temp_store   = MEMORY;
PRAGMA foreign_keys = ON;
```

**Scanning** is walk → filter by extension → read tags → upsert, with `jwalk` and `rayon` for traversal, `lofty` for tags, and `notify` for incremental file watching. The writer drains scan results into one transaction per batch.

---

## Scripting

[ADR 0010](adr/0010-one-script-language.md). `aeri-script` parses a script once into a tree and evaluates it per track. Two entry points share a lexer, a field model, and a function library:

| Entry point | Yields | Used by |
| --- | --- | --- |
| Title formatting | A string | Playlist columns, grouping and sort keys, status bar, window title |
| Query | A boolean | Search, library filters, autoplaylists |

The syntax follows foobar2000's. The crate does no I/O and never panics on any input.

---

## Interface

[ADR 0011](adr/0011-gpui-and-layout-as-data.md).

- **Layout is data.** The window is a tree of splitters, tab stacks, and widgets. Each widget is named by registry id and carries its own settings. The tree is a file; the default interface is one such file.
- **Widgets** are the unit of interface: playlist view, library tree, filters, search, transport controls, seekbar, cover art, status bar. Each depends on `Player` and nothing else in the application.
- **The playlist view is the centre.** Its columns, grouping, and sorting are scripts.
- **Editing the layout** inside the application follows the first GUI release.

GPUI runs its own executor and awaits `Player` on it.

---

## Platform integration

`aeri-platform` is a frontend (ADR 0006): it sends commands through `Player` and renders its state. The binary starts it.

| Platform | Integration | Crate |
| --- | --- | --- |
| Linux | MPRIS D-Bus service | `zbus` |
| macOS | MediaPlayer framework (Now Playing, remote commands) | `objc2` |

---

## Tech stack

Versions are chosen when each dependency is first used, not here.

| Concern | Choice | Notes |
| --- | --- | --- |
| Audio output | Native backends behind `OutputSink` | No `cpal` (ADR 0003) |
| Linux audio | `pipewire` | PipeWire only |
| macOS audio | `coreaudio-rs` | Hog mode for bit-perfect exclusive access |
| Decode | `symphonia` | Pure Rust; FLAC and MP3 features only (ADR 0007) |
| Resample | `rubato` | Sinc interpolation |
| Ring buffer | `rtrb` | Lock-free SPSC; real-time safe |
| Channels | `tokio::sync` | `mpsc`, `oneshot`, `watch`; no runtime required |
| Tags | `lofty` | Read and write |
| Filesystem watch | `notify` | |
| Parallel scan | `jwalk` + `rayon` | |
| Database | `rusqlite` | Bundled SQLite; one writer (ADR 0008) |
| MPRIS | `zbus` | |
| macOS platform | `objc2` | |
| GUI | GPUI + `gpui-component` | Added in the GUI phase (ADR 0011) |
| Errors | `thiserror` (libraries) · `anyhow` (binaries) | |
| Observability | `tracing` + `tracing-subscriber` | Never on the audio thread |
| Distribution | `cargo-dist` | Signs, notarizes, publishes |

---

## Platform targets

| Target | Notes |
| --- | --- |
| `aarch64-apple-darwin` | Minimum macOS 15 (Sequoia), set by `MACOSX_DEPLOYMENT_TARGET` in `.cargo/config.toml` and built in CI on that version |
| `x86_64-unknown-linux-gnu` | Any distribution with PipeWire |
| `aarch64-unknown-linux-gnu` | Same |

Out of scope permanently: Windows, Intel macOS, PulseAudio, raw ALSA, JACK.

---

## Open decisions

Resolve each before the crate that depends on it is written.

- **Metadata schema.** The exact `Track` type, tag-name normalisation, and SQLite schema. Start of the library phase (ADR 0009).
- **Script compatibility.** Which foobar2000 functions and query operators are supported, and where aeri deliberately differs. Start of the scripting phase.
- **GPUI verification.** Large tables, a deserialised splitter tree, accessibility, Linux requirements. Start of the GUI phase (ADR 0011).
- **Layout file format.** Start of the GUI phase.
- **Settings storage.** One store for application settings, widget settings, and DSP chains: SQLite or a file. Decide when the first setting exists.
- **Controlling a running instance.** `aeri pause` from a shell needs a way to reach the running player. MPRIS covers Linux. Decide before the CLI grows beyond `play`.
- **DSP buffer layout.** Interleaved or planar inside the chain. Decide with `aeri-dsp`.
- **GUI visual language.** Typography, spacing, colour. During the GUI phase.
- **Third-party notices.** How each release artefact ships dependency licences and points to MPL source. Before the first release (ADR 0014).

### Resolved

| Decision | Outcome | Record |
| --- | --- | --- |
| Scope | A foobar2000-class desktop player; no daemon, no network API | [ADR 0001](adr/0001-desktop-player-scope.md) |
| Extensibility | Traits with registries, compiled in; no dynamic plugins | [ADR 0002](adr/0002-compiled-in-extension-points.md) |
| Audio output | Native CoreAudio and PipeWire sinks; bit-perfect and gapless scoped | [ADR 0003](adr/0003-native-output-backends.md) |
| Concurrency | Actors; state on a watch channel, events on their own | [ADR 0004](adr/0004-engine-is-an-actor.md) |
| Engine scope | A current track and a next slot; playlists live above | [ADR 0005](adr/0005-engine-plays-current-and-next.md) |
| Frontend seam | One concrete `Player` handle; platform integrations are frontends | [ADR 0006](adr/0006-frontends-use-the-player-handle.md) |
| Decoding | symphonia; FLAC and MP3 first; no C codec | [ADR 0007](adr/0007-symphonia-flac-mp3-first.md) |
| Library | SQLite for persistence, an in-memory store for queries | [ADR 0008](adr/0008-library-sqlite-and-in-memory-store.md) |
| Metadata | Open, multi-value tag map; schema waits for the library | [ADR 0009](adr/0009-open-tag-metadata.md) |
| Scripting | One language for formatting and queries, in its own crate | [ADR 0010](adr/0010-one-script-language.md) |
| GUI | GPUI; layout is data | [ADR 0011](adr/0011-gpui-and-layout-as-data.md) |
| Filesystem errors | All file access through `fs-err`, so errors name their path | [ADR 0012](adr/0012-filesystem-through-fs-err.md) |
| Property tests | `proptest`, std only, regressions committed | [ADR 0013](adr/0013-property-tests-with-proptest.md) |
| Decoder licence | MPL-2.0 allowed for symphonia's crates only; notices required before release | [ADR 0014](adr/0014-mpl-2-0-for-symphonia.md) |
