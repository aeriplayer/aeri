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
   audio thread. Sub-millisecond latency in the hot path.
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
aeri-audio        (cpal + RT thread + OutputSink trait)
aeri-decode       (symphonia adapter)
aeri-dsp          (DSP nodes + rubato resampler)
aeri-library      (SQLite/FTS5 + lofty + scanning)
aeri-platform     (MPRIS / MediaPlayer framework)
    ↑
aeri-engine       (composes the four above)
    ↑
aeri-ipc          (Player trait + LocalPlayer + RemotePlayer + Connect-RPC service)
    ↑
aeri-cli          (binary: `aeri`, daemon mode)
aeri-gui          (binary: `aeri-gui`, iced)

aeri-stream       (post-v1 — HTTP streaming + Subsonic API, depends on aeri-ipc)
```

### Dependency rules

- `aeri-core` has **no** dependencies on other workspace crates.
- `aeri-cli` and `aeri-gui` depend on `aeri-ipc`, **never on `aeri-engine` directly**.
  The `Player` trait is the contract; bypassing it erodes the composability pillar.
- `aeri-platform` is wired in by `aeri-engine`; frontends do not reference it.
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

### IPC protocol

The service is defined in `proto/aeri/v1/player.proto`. Code generation runs in
`aeri-ipc`'s `build.rs` via `buffa-build` (message types) and `connectrpc-build`
(Tower service stubs). `protoc` must be installed; the build script warns and skips
gracefully if it is not.

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
                 │ SPSC command queue
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
└─────────────────────────────────────────┘

Rayon pool — parallel directory scans, CPU-bound batch work
```

**The audio thread never touches tokio.** This is an absolute constraint, not a
guideline. The two crossing points are:

- **`rtrb` SPSC ring buffer** — audio samples from decode/DSP to output thread.
- **SPSC command queue** — control messages (seek, pause, volume) from the tokio side
  to the audio thread.

`unsafe` is permitted only in `aeri-audio`, specifically for FFI and the real-time
SPSC bridge. Every `unsafe` block carries a `// SAFETY:` comment.

---

## DSP pipeline

Linear. Not a DAG. The DAG constraint is load-bearing: it keeps latency predictable,
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
| Block size | 1024 frames | Balance between latency and per-block overhead |
| Sample rate behavior | Follow source; re-init device on change | Bit-perfect: device must match source |
| Gapless playback | Always on | Core feature; pre-roll makes it free |
| Crossfade | Not supported | Fundamentally incompatible with strict gapless |
| ReplayGain | On by default, track + album modes | |
| Volume control | Software, in DSP chain | Hardware volume disabled by default for bit-perfect |
| Resampler | `rubato` (sinc interpolation, high quality) | Best pure-Rust option; sinc avoids aliasing |

---

## Library layer

The library crate manages the music database: scanning, tag ingestion, and search.

### Database

SQLite with WAL mode. All writes and long-running reads happen via `deadpool-sqlite`,
which maintains an async connection pool backed by `spawn_blocking`. The pool eliminates
the manual `spawn_blocking` wrapping that would otherwise be required at every call site.

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

SQLite FTS5 with a contentless virtual table. The virtual table mirrors
`title / artist / album / album_artist / composer` from the main tracks table.
Contentless means no storage duplication; the main table is the source of truth.

Browsing queries use **keyset pagination**, not `OFFSET`. `OFFSET` forces a full
prefix scan on large libraries; keyset pagination is O(log n) with the right index.

### Scanning

`jwalk` + `rayon` for parallel directory traversal. `lofty` for tag read/write.
`notify` (v8) for incremental filesystem watch. The scan pipeline is:
walk → filter audio extensions → read tags via lofty → upsert into SQLite.

---

## Platform integration

`aeri-platform` is compiled into `aeri-engine` and wired up at engine initialization.
Frontends do not reference it directly.

| Platform | Integration | Crate |
| --- | --- | --- |
| Linux | MPRIS D-Bus service | `zbus` |
| macOS | MediaPlayer framework (Now Playing, remote commands) | `objc2` |

---

## Tech stack

| Concern | Choice | Notes |
| --- | --- | --- |
| Audio output | `cpal` | Direct host API access; not `rodio` (too high-level) |
| Linux audio backend | `pipewire` | PipeWire only — no PulseAudio, no raw ALSA |
| macOS audio backend | `coreaudio-rs` | Hog mode for bit-perfect exclusive access |
| Decode | `symphonia` | Pure Rust; best-in-class format coverage |
| Resample | `rubato` | Sinc interpolation; only serious pure-Rust option |
| Tag read/write | `lofty` | Handles all target formats; active maintenance |
| Filesystem watch | `notify` | Cross-platform; async-friendly |
| Parallel scan | `jwalk` + `rayon` | jwalk streams results during traversal |
| Database | `rusqlite` + `deadpool-sqlite` | Async pool; bundled SQLite for reproducibility |
| Search | SQLite FTS5 | Embedded, zero infrastructure, sufficient at library scale |
| Async runtime | `tokio` | Ecosystem standard |
| Ring buffer | `rtrb` | Lock-free SPSC; RT-safe |
| RPC | `connectrpc` | Connect protocol (HTTP/1.1 compatible, browser/mobile friendly) |
| Protobuf | `buffa` | Pure Rust; first-class proto editions support |
| MPRIS | `zbus` | Idiomatic D-Bus for Rust |
| macOS platform | `objc2` | Safe Objective-C bindings |
| GUI | `iced` | Rust-native; appropriate for a polished consumer app |
| Errors | `thiserror` (libraries) · `anyhow` (binaries) | Standard split |
| Observability | `tracing` + `tracing-subscriber` | Structured, async-aware |
| Distribution | `cargo-dist` | Signs, notarizes, publishes across all targets |

### Format support (v0.1 floor)

MP3, FLAC, Ogg Vorbis, Opus, WAV, AIFF. AAC/ALAC deferred until stable symphonia
support lands. No Monkey's Audio, TAK, WavPack, or DSD.

---

## Output sink abstraction (streaming seam)

This seam must be in place before any audio code is written.

`aeri-audio` defines an `OutputSink` trait. `aeri-engine` routes decoded, DSP-processed
audio to one or more sinks. In v0–v1 the only implementation is the local cpal sink.
In v1.x a second implementation in `aeri-stream` will encode to Opus and write to an
HTTP response body.

The seam keeps v1.x streaming from requiring engine changes. If the trait is missing
and audio is hardwired to cpal, streaming requires invasive surgery later.

### Streaming roadmap

| Phase | Scope |
| --- | --- |
| v0–v1 | Desktop only. `OutputSink` trait in place; only cpal sink implemented |
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
  Design before any `aeri-library` code.
- **Daemon concurrency** — single-client or multiple concurrent clients. Design before
  `aeri-ipc` service methods are defined.
- **Smart playlist DSL** — deferred to v0.2.
- **iced visual language** — typography, spacing, colour system. TBD during v0 GUI phase.
