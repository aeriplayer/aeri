# aeri — Architecture

This document is the authoritative reference for system design, structural constraints,
and non-obvious decisions. Process, conventions, and build instructions live in
`CLAUDE.md`. New architectural decisions should be recorded here as ADRs in
`docs/adr/`.

---

## Thesis

aeri is a second-mover successor to foobar2000. The bet: the decade-old players were
designed around the constraints of the early 2000s (single-threaded decoders, plugin
ecosystems as distribution, sync I/O everywhere). Designed fresh in 2026, the right
architecture looks quite different — a headless engine exposed over a typed RPC
interface, a linear DSP pipeline with immutable locked-in decisions, and a deployment
model built for Apple Silicon and PipeWire from the start.

---

## Non-negotiable pillars

These are load-bearing constraints. Violating them makes the architecture incoherent.

1. **Modern hardware only.** Apple Silicon. PipeWire. No compatibility shims. Targeting
   old hardware trades correctness headroom for zero users.
2. **Composable.** The engine is headless. Every frontend (GUI, CLI, future mobile) is a
   client over the `Player` trait. Frontends never call engine code directly.
3. **Real-time audio thread is sacred.** No allocations, no locks, no tokio on the
   audio thread. The target is zero underruns, not low latency — a music player can
   buffer generously, and should.
4. **No plugins in v0–v1.** Plugin loading is a major-version event, scoped to the WASM
   Component Model when it happens. The attack surface and ABI stability costs are not
   worth it before the core is solid.
5. **Defensible dependencies.** Every crate addition is justified. Prefer tokio-rs,
   dtolnay, rust-lang, and established ecosystem authors. No bleeding-edge bets without
   written rationale.

---

## Crate graph

```text
aeri-core         (domain types — no workspace deps)
    ↑
aeri-audio        (OutputSink trait + native CoreAudio / PipeWire sinks + RT thread)
aeri-decode       (symphonia adapter)
aeri-dsp          (DSP nodes + rubato resampler)                    planned · Phase 5
aeri-library      (SQLite/FTS5 + lofty + scanning)                  planned · Phase 4
    ↑
aeri-engine       (composes the four above)
    ↑
aeri-ipc          (Player trait + RemotePlayer + Connect-RPC service;
                   LocalPlayer and the engine edge sit behind the `local` feature)
    ↑
aeri-platform     (MPRIS / MediaPlayer framework — a Player client) planned · Phase 5
    ↑
aeri-cli          (binary: `aeri`, daemon mode)
aeri-gui          (binary: `aeri-gui`, GPUI)                        planned · Phase 6

aeri-stream       (HTTP streaming + Subsonic API; depends on aeri-ipc) planned · post-v1
```

**A crate is created in the phase that first needs it.** A stub crate still costs
compile time, lockfile entries, and dependency upkeep, and its boundary hardens before
any code has tested it. Crates marked *planned* do not exist in the workspace yet; six
do: `aeri-core`, `aeri-audio`, `aeri-decode`, `aeri-engine`, `aeri-ipc`, `aeri-cli`.

The `Player` boundary is fixed. The split *below* the engine (`aeri-audio`,
`aeri-decode`, `aeri-dsp`) was drawn before any audio flowed and is provisional:
expect it to be reshaped in Phase 3, and do not contort code to preserve it.

### Dependency rules

- `aeri-core` has **no** dependencies on other workspace crates.
- `aeri-cli` and `aeri-gui` depend on `aeri-ipc`, **never on `aeri-engine` directly**.
  The `Player` trait is the contract; bypassing it erodes the composability pillar.
- `aeri-ipc` depends on `aeri-engine` only behind its default-on `local` feature, which
  gates `LocalPlayer`. A remote-only client builds with `default-features = false` and
  links no audio, decode, or database code (ADR 0003).
- `aeri-platform` is a `Player` client (ADR 0006). It depends on `aeri-ipc` without
  `local`, never on `aeri-engine`, and is started by whichever process hosts the
  engine: the daemon, or the GUI in a single-binary build.
- No cyclic dependencies. If you reach for one, the abstraction boundary is wrong.

---

## Headless engine and IPC layer

`aeri-engine` is a library. It is never a process. It can be hosted in-process or
behind a daemon.

`aeri-ipc` exposes two implementations of the `Player` trait:

| Implementation | How it works | When to use |
| --- | --- | --- |
| `LocalPlayer` | Owns an `aeri_engine::Engine` directly | Single-binary desktop build; zero IPC overhead |
| `RemotePlayer` | Connect-RPC client over a Unix socket or TCP | External clients: mobile, web, third-party |

All frontends program against `Player`. Neither the GUI nor the CLI knows or cares which
implementation is active. This is the boundary that makes remote clients possible
without architectural rework.

`LocalPlayer` arrives in Phase 3. `RemotePlayer`, the Connect service, and the daemon
arrive at the start of Phase 4 (ADR 0002). `Player` methods take and return owned
`aeri-core` types only, so the trait stays serialisable while it has no wire form.

### Concurrency

The daemon is multi-client (ADR 0001). Commands from every client funnel into the
engine's single command queue and apply in arrival order; conflicts are
last-writer-wins. State is observed through a subscription on the `Player` trait — a
server-streaming RPC on the wire — never by polling. The subscription is part of the
trait from its first version.

### IPC protocol

The service is defined in `proto/aeri/v1/player.proto`. Code generation runs in
`aeri-ipc`'s `build.rs` via `buffa-build` (message types) and `connectrpc-build`
(Tower service stubs). `protoc` must be installed; the build script warns and skips
gracefully if it is not. The service is intentionally empty until Phase 4; codegen
still runs so the path stays exercised in CI.

**Connect-RPC over gRPC:** Connect works over HTTP/1.1 and is browser-compatible. The
iOS streaming client (v2 roadmap) will use the same API the desktop GUI uses, without
a gRPC-Web proxy layer. This is why Connect was chosen over `tonic`.

### IPC design contract

Library browsing, queue manipulation, and playback control must be first-class IPC
operations — no GUI-specific shortcuts, no assumptions about a co-located caller.
Track metadata returned by the service must be sufficient to render a list row without
further calls: title, artist, album, duration, optional cover-art URL.

---

## Threading model

```text
┌─────────────────────────────────────────┐
│  Tokio runtime                          │
│  IPC · library scans · FS watch · meta  │
└────────────────┬────────────────────────┘
                 │ command channel (play, seek, next, stop)
                 ▼
┌─────────────────────────────────────────┐
│  Decode / DSP thread(s)                 │
│  feeds ring buffer · pre-roll ~500 ms   │
└────────────────┬────────────────────────┘
                 │ rtrb SPSC ring buffer (audio samples)
                 ▼
┌─────────────────────────────────────────┐
│  Audio output thread  (RT priority)     │
│  NO allocations · NO syscalls except    │
│  audio API · NO locks · NO tokio        │
│  reads atomics only (paused, flush)     │
└─────────────────────────────────────────┘

Rayon pool — parallel directory scans, CPU-bound batch work
```

**The audio thread never touches tokio.** This is an absolute constraint, not a
guideline. Control flows like this:

- **Commands go to the decode/DSP thread**, not the audio thread. Seek, track change,
  and stop are decode-side operations: they change what is written into the ring
  buffer. Volume is a DSP node, so it lives there too.
- **The audio callback's only inputs** are the `rtrb` ring buffer and a handful of
  atomics (paused, flush-generation). It never receives a message it would have to
  interpret.
- A seek therefore bumps the flush generation; the callback discards stale samples
  until the generation it reads from the buffer matches.

The design target is **zero underruns**. Latency between a command and its audible
effect is bounded by the pre-roll and is not a goal to minimise.

`unsafe` is permitted only in `aeri-audio`, specifically for FFI and the real-time
SPSC bridge. Every `unsafe` block carries a `// SAFETY:` comment.

---

## DSP pipeline

Linear. Not a DAG. The linear constraint is load-bearing: it keeps latency predictable,
simplifies gapless pre-roll, and eliminates a whole class of topology bugs.

```text
Decoder → Format Convert → DSP Chain → Resampler → OutputSink
                (→ f32 planar)    ↑
                         [ReplayGain, EQ, ...]
```

### Locked decisions

These are not up for reconsideration without an ADR.

| Decision | Value | Rationale |
| --- | --- | --- |
| Internal sample format | `f32` planar | Sufficient precision; avoids per-node format negotiation |
| Block size | 1024 frames (internal) | Balance between latency and per-block overhead. This is the DSP block size, **not** the device period — the callback buffer size is chosen by the host audio API and must not be forced |
| Sample rate behavior | Follow source; re-init output on change | Bit-perfect on macOS (hog mode). Best-effort on Linux, where PipeWire owns the graph rate; the negotiated rate is reported, never assumed (ADR 0004) |
| Gapless playback | Always on, across transitions that keep rate and channel layout | Core feature; pre-roll makes it free. A format change re-inits the output and that gap is accepted (ADR 0004) |
| Crossfade | Not supported in v1 | Scope, not incompatibility — see note below |
| ReplayGain | On by default, track + album modes | |
| Volume control | Software, in DSP chain | Hardware volume disabled by default for bit-perfect |
| Resampler | `rubato` (sinc interpolation, high quality) | Best pure-Rust option; sinc avoids aliasing. Bypassed on the normal path — see note below |

#### On the resampler being bypassed

"Follow source; re-init output on change" means the output rate normally *equals* the
source rate, so the resampler does no work on the common path. It exists for two cases:

1. **A macOS device refuses the rate.** Fall back to the nearest supported rate and
   resample. (On Linux, PipeWire resamples in the server when it declines a rate, so
   aeri's resampler is not involved.)
2. **The v1.x Opus stream sink.** Opus is 48 kHz only, so the streaming path always
   resamples regardless of source.

Without this note the terminal DSP node reads as dead code.

#### On crossfade

Crossfade and gapless are not mutually exclusive — they are per-transition behaviours,
and players like foobar2000 ship both. Crossfade is excluded from v1 because it costs a
second decoder, an overlap buffer, and a policy for which transitions get faded; that is
scope we are not spending. Revisit on those grounds, not on a false technical conflict.

---

## Library layer

The library crate manages the music database: scanning, tag ingestion, and search.

### Database

SQLite with WAL mode, on `rusqlite` directly — no pool crate (ADR 0005).

- **One writer connection** on a dedicated thread, fed by an mpsc channel of write
  jobs. Every mutation goes through it. SQLite serialises writes regardless, so a pool
  of writers would buy `SQLITE_BUSY`, not concurrency.
- **A small fixed set of read-only connections**, handed out per query and run via
  `spawn_blocking`.

A caller that needs read-your-writes awaits the write job before querying.

**Required PRAGMAs** — applied on every new connection:

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous  = NORMAL;
PRAGMA cache_size   = -65536;      -- 64 MB page cache
PRAGMA mmap_size    = 268435456;   -- 256 MB mmap window
PRAGMA temp_store   = MEMORY;
PRAGMA foreign_keys = ON;
```

### Search

SQLite FTS5 with an **external-content** virtual table
(`content='tracks', content_rowid='id'`), indexing
`title / artist / album / album_artist / composer` from the main tracks table.
External content means no storage duplication — FTS5 reads column values back from
`tracks`, which stays the source of truth.

Not *contentless* (`content=''`): that stores no recoverable text at all, which
breaks `highlight()` and `snippet()` (both return empty) and forces every delete to
hand back the original column values. Verified on SQLite 3.51: a contentless table
returns `''` from `highlight()` where an external-content table returns the marked-up
match.

Browsing queries use **keyset pagination**, not `OFFSET`. `OFFSET` forces a full
prefix scan on large libraries; keyset pagination is O(log n) with the right index.

### Scanning

`jwalk` + `rayon` for parallel directory traversal. `lofty` for tag read/write.
`notify` (v8) for incremental filesystem watch. The scan pipeline is:
walk → filter audio extensions → read tags via lofty → upsert into SQLite.

---

## Platform integration

`aeri-platform` is a `Player` client (ADR 0006). MPRIS and the macOS remote command
centre issue transport commands and display now-playing state, which makes them
frontends: they send commands through `Player` and render its state subscription. The
process hosting the engine starts them; a remote-only client never does.

| Platform | Integration | Crate |
| --- | --- | --- |
| Linux | MPRIS D-Bus service | `zbus` |
| macOS | MediaPlayer framework (Now Playing, remote commands) | `objc2` |

---

## Tech stack

| Concern | Choice | Notes |
| --- | --- | --- |
| Audio output | Native backends behind `OutputSink` | No `cpal`: it hides hog mode and rate switching, and reaches Linux devices via ALSA (ADR 0004) |
| Linux audio backend | `pipewire` | PipeWire only — no PulseAudio, no raw ALSA. Bit-perfect is best-effort |
| macOS audio backend | `coreaudio-rs` | Hog mode for bit-perfect exclusive access |
| Decode | `symphonia` | Pure Rust; best-in-class format coverage. Opus goes through libopus, the one C codec (ADR 0008) |
| Resample | `rubato` | Sinc interpolation; only serious pure-Rust option |
| Tag read/write | `lofty` | Handles all target formats; active maintenance |
| Filesystem watch | `notify` | Cross-platform; async-friendly |
| Parallel scan | `jwalk` + `rayon` | jwalk streams results during traversal |
| Database | `rusqlite` | One writer + WAL readers, owned directly (ADR 0005); bundled SQLite for reproducibility |
| Search | SQLite FTS5 | Embedded, zero infrastructure, sufficient at library scale |
| Async runtime | `tokio` | Ecosystem standard |
| Ring buffer | `rtrb` | Lock-free SPSC; RT-safe |
| RPC | `connectrpc` | Connect protocol (HTTP/1.1 compatible, browser/mobile friendly) |
| Protobuf | `buffa` | Pure Rust; first-class proto editions support |
| MPRIS | `zbus` | Idiomatic D-Bus for Rust |
| macOS platform | `objc2` | Safe Objective-C bindings |
| GUI | GPUI + `gpui-component` | Built for large virtualised lists; dependency added in Phase 6 (ADR 0007) |
| Errors | `thiserror` (libraries) · `anyhow` (binaries) | Standard split |
| Observability | `tracing` + `tracing-subscriber` | Structured, async-aware |
| Distribution | `cargo-dist` | Signs, notarizes, publishes across all targets |

### Format support (v0.1 floor)

MP3, FLAC, Ogg Vorbis, Opus, WAV, AIFF, AAC, ALAC (the last two in MP4/M4A) — see
ADR 0008. No Monkey's Audio, TAKE, WavPack, or DSD. No DRM-protected files.

---

## Output sink abstraction (streaming seam)

This seam must be in place before any audio code is written.

`aeri-audio` defines an `OutputSink` trait. `aeri-engine` routes decoded, DSP-processed
audio to one or more sinks. In v0–v1 the implementations are the two native device sinks and `FakeSink`.
In v1.x a second implementation in `aeri-stream` will encode to Opus and write to an
HTTP response body.

The seam keeps v1.x streaming from requiring engine changes. If the trait is missing
and audio is hardwired to a device API, streaming requires invasive surgery later.

**Open caveat.** A sink placed *after* the DSP chain receives audio with local
ReplayGain, EQ, and volume already applied, locked to the local device's sample rate.
That is right for "cast what I am hearing" and wrong for Subsonic clients, which expect
to transcode from the source file and apply their own gain. Expect the streaming path to
tap the *source*, not the post-DSP tee. The trait is still worth having; the v1.x plan
resting on it needs revisiting before `aeri-stream` is written.

### Streaming roadmap

| Phase | Scope |
| --- | --- |
| v0–v1 | Desktop only. `OutputSink` trait in place; only device sinks implemented |
| v1.x | `aeri-stream` crate: HTTP streaming + Subsonic-compatible API, mDNS/Bonjour discovery, pairing auth |
| v2 / separate repo | `aeri-ios`: Connect Swift SDK for control, AVPlayer for audio |

The Subsonic-compatible API is the practical unlock: it makes every existing iOS Subsonic
client (Amperfy, play:Sub, substreamer) work against the desktop library without an
aeri-native iOS app.

---

## Platform targets

| Target | Notes |
| --- | --- |
| `aarch64-apple-darwin` | Minimum macOS 14 (Sonoma) |
| `x86_64-unknown-linux-gnu` | Any distro with PipeWire |
| `aarch64-unknown-linux-gnu` | Same |

Out of scope permanently: Windows, Intel macOS, PulseAudio, raw ALSA.

---

## Open decisions

These must be resolved before the relevant crate is written.

- **Metadata schema** — exact shape of tracks, albums, artists, tags, and custom fields.
  Designed at the start of Phase 4 with the SQLite schema and library RPCs that consume
  it (ADR 0009). Until then `aeri-core` carries playback types only.
- **State fan-out** — how the multi-client daemon (ADR 0001) delivers state to
  subscribers: initial snapshot on subscribe, slow-subscriber policy. Design before the
  Phase 4 wire protocol.
- **Smart playlist DSL** — deferred to v0.2.
- **GUI visual language** — typography, spacing, colour system on GPUI. TBD during the
  v0 GUI phase.
- **GPUI verification** — accessibility support, Linux requirements, and a 50,000-row
  list, checked at the start of Phase 6 before screens are built (ADR 0007).
- **Streaming tap point** — see the open caveat under "Output sink abstraction".
  Decide before `aeri-stream` is written.

### Resolved

| Decision | Outcome | Record |
| --- | --- | --- |
| Daemon concurrency | Multi-client; last-writer-wins commands, subscribed state | [ADR 0001](adr/0001-multi-client-daemon.md) |
| Wire protocol timing | Phase 3 ships `LocalPlayer` only; protocol and daemon in Phase 4 | [ADR 0002](adr/0002-defer-wire-protocol.md) |
| `aeri-ipc` → `aeri-engine` | Optional, behind default-on `local` feature | [ADR 0003](adr/0003-ipc-local-feature.md) |
| Audio output layer | Native CoreAudio and PipeWire sinks, no cpal; bit-perfect and gapless claims scoped | [ADR 0004](adr/0004-native-output-backends.md) |
| Database connections | One writer + WAL readers on `rusqlite`; `deadpool-sqlite` dropped | [ADR 0005](adr/0005-sqlite-single-writer.md) |
| `aeri-platform` placement | A `Player` client started by the engine's host process | [ADR 0006](adr/0006-platform-integrations-are-player-clients.md) |
| GUI toolkit | GPUI, replacing iced | [ADR 0007](adr/0007-gpui-for-the-gui.md) |
| Format floor | Adds AAC and ALAC; Opus via libopus is the one C codec | [ADR 0008](adr/0008-format-floor-aac-alac.md) |
| Metadata schema timing | Phase 4, not Phase 2 | [ADR 0009](adr/0009-defer-metadata-schema.md) |
