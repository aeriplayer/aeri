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

Five of the scaffolded crates (`aeri-library`, `aeri-dsp`, `aeri-platform`, `aeri-gui`,
`aeri-stream`) were later removed as empty stubs. Each is re-created in the phase that
first needs it, noted below.

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

## Phase 2 — Core playback types

> `aeri-core` defines what playback needs, and nothing more. The metadata schema waits
> for Phase 4, where its consumers are ([ADR 0009](adr/0009-defer-metadata-schema.md)).

No blockers. Small enough to fold into the start of Phase 3 work.

- [x] Finalize daemon concurrency model: multi-client
      ([ADR 0001](adr/0001-multi-client-daemon.md))
- [ ] `PlaybackState`: stopped, playing, paused, buffering; position + duration
- [ ] `Queue`
- [ ] Minimal `Track`: source location, duration, optional title / artist / album
- [ ] Unit tests for domain type invariants

---

## Phase 3 — Minimal end-to-end

> `aeri play file.flac` produces audio. The engine, decoder, and `Player` trait are
> proven to compose. No wire protocol or daemon yet
> ([ADR 0002](adr/0002-defer-wire-protocol.md)).

- [ ] `OutputSink` trait in `aeri-audio` — the v1.x streaming seam; do not skip
- [ ] `FakeSink` implementation for tests (captures samples, asserts on output)
- [ ] `aeri-decode`: symphonia adapter behind a `Decoder` trait; FLAC + MP3 first
- [ ] `aeri-audio`: native sink for the development platform first (`CoreAudioSink`),
      real-time output thread, `rtrb` ring buffer, atomics for paused / flush
      ([ADR 0004](adr/0004-native-output-backends.md))
- [ ] `aeri-audio`: `PipeWireSink`; report the negotiated rate
- [ ] Command channel into the decode thread: play, seek, stop
- [ ] `aeri-engine`: wires decoder → DSP chain → resampler → `OutputSink`
- [x] `aeri-ipc`: `aeri-engine` optional behind default-on `local` feature
      ([ADR 0003](adr/0003-ipc-local-feature.md))
- [ ] `aeri-ipc`: `Player` trait (play, pause, stop, seek, state subscription; owned
      `aeri-core` types only); `LocalPlayer` wrapping the engine
- [ ] `aeri-cli`: `play <path>` in the foreground, driving `LocalPlayer`
- [ ] Integration test: engine plays a fixture FLAC through `FakeSink`, asserts on
      sample count and silence-free output

---

## Phase 4 — Library

> Users can point aeri at a music directory and browse/search it. The daemon and wire
> protocol land first, since the library RPCs need them.

- [ ] Create the `aeri-library` crate; add `rusqlite`, `lofty`, `jwalk`, `rayon`,
      `notify` at then-current versions
- [ ] Metadata schema: `Track`, `Album`, `Artist`, custom tag fields — designed with
      the SQLite schema below; `aeri-core` types grow to match
      ([ADR 0009](adr/0009-defer-metadata-schema.md))
- [ ] State fan-out design: initial snapshot, slow-subscriber policy
- [ ] `proto/aeri/v1/player.proto`: `Play`, `Pause`, `Stop`, `Seek`, server-streaming
      `WatchState`; Connect service + `RemotePlayer` in `aeri-ipc`
- [ ] `aeri-cli`: `daemon`, plus `pause`, `stop`, `seek <seconds>` over `RemotePlayer`
- [ ] SQLite schema + required PRAGMAs applied on every new connection
- [ ] Connection layer: one writer thread fed by a channel + read-only WAL
      connections ([ADR 0005](adr/0005-sqlite-single-writer.md))
- [ ] Tag ingestion pipeline: `jwalk` walk → extension filter → `lofty` read → upsert
- [ ] FTS5 external-content virtual table: `title / artist / album / album_artist / composer`
- [ ] Keyset-paginated browse queries (no `OFFSET`)
- [ ] `notify` filesystem watcher for incremental rescans
- [ ] IPC RPCs: `ScanLibrary`, `ListTracks`, `SearchTracks`, `GetAlbum`, `ListAlbums`
- [ ] `tests/fixtures/`: small FLAC, MP3, Ogg, M4A (AAC + ALAC) files covering
      edge-case tag encodings
- [ ] Integration tests: round-trip tag reads, FTS query coverage, scan idempotency

---

## Phase 5 — DSP chain and platform integration

> Full DSP chain with ReplayGain; bit-perfect path when DSP is bypassed. Platform OS
> integration active.

- [ ] Create the `aeri-dsp` and `aeri-platform` crates; add `rubato`, `zbus`, `objc2`
- [ ] `aeri-dsp`: DSP node trait; `ReplayGainNode`, `VolumeNode`, `EqualizerNode`
- [ ] `rubato` sinc resampler wired as the terminal DSP node
- [ ] ReplayGain on by default; track and album modes; R128 loudness fallback
- [ ] `aeri-platform`, as a `Player` client started by the engine's host process
      ([ADR 0006](adr/0006-platform-integrations-are-player-clients.md)):
  - Linux: MPRIS D-Bus service via `zbus` (play/pause/next/prev/metadata)
  - macOS: MediaPlayer Now Playing + remote command center via `objc2`
- [ ] Format support floor complete: MP3, FLAC, Ogg Vorbis, Opus, WAV, AIFF, AAC, ALAC
      ([ADR 0008](adr/0008-format-floor-aac-alac.md)); confirm AAC profile coverage
      with fixtures
- [ ] Gapless playback verified end-to-end via integration test (same-format
      transitions); format-change re-init verified separately

---

## Phase 6 — First GUI (v0.1 milestone)

> Usable by the developer. Not yet polished. Ships as a signed binary.

- [ ] GPUI verification before any screens: accessibility support, Linux
      requirements, 50,000-row track list with live search
      ([ADR 0007](adr/0007-gpui-for-the-gui.md))
- [ ] Create the `aeri-gui` crate: GPUI application shell; channel bridge to `Player`
- [ ] Now-playing panel: cover art, title/artist/album, position scrubber, transport
- [ ] Queue panel: ordered list, drag-to-reorder, remove item
- [ ] Library panel: album grid, track list, search box wired to FTS5
- [ ] macOS: `.dmg` via `cargo-dist`; signed + notarized
- [ ] Linux: AUR `PKGBUILD` draft
- [ ] Visual language decision: typography, spacing, colour system

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
