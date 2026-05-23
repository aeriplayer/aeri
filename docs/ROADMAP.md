# aeri — Roadmap

Phases track progress from scaffolding through stable desktop to streaming and mobile.
Architectural decisions that gate specific phases are noted inline; their rationale lives
in [`ARCHITECTURE.md`](ARCHITECTURE.md).

---

## Phase 0 — Scaffolding ✓

> Workspace exists, all crates compile, dependency graph is locked.

- [x] Initialize Cargo workspace; `cargo new --lib` for all crates
- [x] Wire `[workspace.dependencies]` and `[workspace.lints]`
- [x] `rust-toolchain.toml`, `rustfmt.toml`, `dist-workspace.toml`
- [x] `LICENSE-MIT`, `LICENSE-APACHE`, `COPYRIGHT`
- [x] `proto/aeri/v1/player.proto` stub; `aeri-ipc` build script with `buffa-build` +
      `connectrpc-build`
- [x] `docs/ARCHITECTURE.md`

---

## Phase 1 — Infrastructure ✓

> CI green on all targets; release pipeline wired.

- [x] GitHub Actions matrix: `aarch64-apple-darwin`, `x86_64-unknown-linux-gnu`,
      `aarch64-unknown-linux-gnu`
  - Jobs: `cargo check`, `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check`
- [x] `cargo-dist` skeleton — installers, artifact upload, release draft
- [x] `git init`, initial commit
- [x] Push to remote

---

## Phase 2 — Core domain types

> `aeri-core` is fully defined. Everything downstream can reference stable types.

**Blocker:** resolve the metadata schema open decision before writing any library code.
The schema shapes every query, every IPC message, and every GUI binding — changing it
later is expensive.

- [ ] Finalize metadata schema: `Track`, `Album`, `Artist`, custom tag fields
- [ ] Finalize daemon concurrency model: single-client vs. multi-client
      (shapes `aeri-ipc` service design)
- [ ] Implement `Track`, `Album`, `Artist`, `Queue`, `PlaybackState` in `aeri-core`
- [ ] `PlaybackState` covers: stopped, playing, paused, buffering; position + duration
- [ ] Unit tests for domain type invariants

---

## Phase 3 — Minimal end-to-end

> `aeri-cli play file.flac` produces audio. The engine, decoder, and IPC layer are
> proven to compose.

- [ ] `OutputSink` trait in `aeri-audio` — the v1.x streaming seam; do not skip
- [ ] `FakeSink` implementation for tests (captures samples, asserts on output)
- [ ] `aeri-decode`: symphonia adapter behind a `Decoder` trait; FLAC + MP3 first
- [ ] `aeri-audio`: cpal host, real-time output thread, `rtrb` ring buffer,
      SPSC command queue
- [ ] `aeri-engine`: wires decoder → DSP chain → resampler → `OutputSink`
- [ ] `aeri-ipc`: `Player` trait; `LocalPlayer` wrapping the engine;
      `proto/aeri/v1/player.proto` — `Play`, `Pause`, `Stop`, `Seek` RPCs at minimum
- [ ] `aeri-cli`: `play <path>`, `pause`, `stop`, `seek <seconds>`
- [ ] Integration test: engine plays a fixture FLAC through `FakeSink`, asserts on
      sample count and silence-free output

---

## Phase 4 — Library

> Users can point aeri at a music directory and browse/search it.

- [ ] SQLite schema + required PRAGMAs applied on every new connection
- [ ] `deadpool-sqlite` pool initialisation in `aeri-library`
- [ ] Tag ingestion pipeline: `jwalk` walk → extension filter → `lofty` read → upsert
- [ ] FTS5 contentless virtual table: `title / artist / album / album_artist / composer`
- [ ] Keyset-paginated browse queries (no `OFFSET`)
- [ ] `notify` filesystem watcher for incremental rescans
- [ ] IPC RPCs: `ScanLibrary`, `ListTracks`, `SearchTracks`, `GetAlbum`, `ListAlbums`
- [ ] `tests/fixtures/`: small FLAC, MP3, Ogg files covering edge-case tag encodings
- [ ] Integration tests: round-trip tag reads, FTS query coverage, scan idempotency

---

## Phase 5 — DSP chain and platform integration

> Bit-perfect playback with ReplayGain. Platform OS integration active.

- [ ] `aeri-dsp`: DSP node trait; `ReplayGainNode`, `VolumeNode`, `EqualizerNode`
- [ ] `rubato` sinc resampler wired as the terminal DSP node
- [ ] ReplayGain on by default; track and album modes; R128 loudness fallback
- [ ] `aeri-platform`:
  - Linux: MPRIS D-Bus service via `zbus` (play/pause/next/prev/metadata)
  - macOS: MediaPlayer Now Playing + remote command center via `objc2`
- [ ] Format support floor complete: MP3, FLAC, Ogg Vorbis, Opus, WAV, AIFF
- [ ] Gapless playback verified end-to-end via integration test

---

## Phase 6 — First GUI (v0.1 milestone)

> Usable by the developer. Not yet polished. Ships as a signed binary.

- [ ] `aeri-gui` skeleton: iced application shell, tokio executor
- [ ] Now-playing panel: cover art, title/artist/album, position scrubber, transport
- [ ] Queue panel: ordered list, drag-to-reorder, remove item
- [ ] Library panel: album grid, track list, search box wired to FTS5
- [ ] macOS: `.dmg` via `cargo-dist`; signed + notarized
- [ ] Linux: AUR `PKGBUILD` draft
- [ ] `iced` visual language decision: typography, spacing, colour system

---

## v1.0 — Stable desktop release

> Polished, documented, publicly available. Homebrew tap live.

- [ ] Smart playlist DSL (design deferred to this milestone)
- [ ] Tag editor in GUI
- [ ] Full keyboard navigation
- [ ] Accessibility audit
- [ ] Homebrew tap (`pkg/homebrew/`)
- [ ] AUR publish
- [ ] `cargo-dist` release with Sigstore signing + GitHub provenance attestations

---

## v1.x — Streaming and remote clients

> Play your desktop library on a phone over LAN. No new iOS app required.

The `OutputSink` trait (Phase 3) is the seam that makes this possible without engine
changes. If it was skipped, this phase requires invasive rework.

- [ ] `aeri-stream` crate: `StreamSink` implementing `OutputSink`; encodes to Opus,
      feeds HTTP response body
- [ ] Subsonic-compatible REST API (unlocks Amperfy, play:Sub, substreamer on iOS)
- [ ] Progressive HTTP and HLS streaming endpoints
- [ ] mDNS/Bonjour LAN discovery
- [ ] Pairing auth: displayed-code flow on desktop, token issued to client
- [ ] IPC metadata contract verified: every track row renderable without a follow-up call

---

## v2 — Native iOS client

> `aeri-ios` — native app using Connect for control, AVPlayer for audio.

- [ ] Separate repo: `aeri-ios`
- [ ] Connect Swift SDK for `PlayerService` RPC
- [ ] AVPlayer consuming the HTTP audio stream from `aeri-stream`
- [ ] Library browse and search via the same IPC APIs the desktop GUI uses
