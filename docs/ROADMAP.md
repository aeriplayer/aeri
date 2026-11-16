# aeri — Roadmap

Phases run from scaffolding to a foobar2000-class desktop player ([ADR 0001](adr/0001-desktop-player-scope.md)). Each phase ends in something usable. Rationale lives in [`ARCHITECTURE.md`](ARCHITECTURE.md) and the ADRs.

---

## Phase 0 — Scaffolding ✓

> Workspace exists, all crates compile.

- [x] Cargo workspace, `[workspace.dependencies]`, `[workspace.lints]`
- [x] `rust-toolchain.toml`, `rustfmt.toml`, `dist-workspace.toml`
- [x] `LICENSE-MIT`, `LICENSE-APACHE`, `COPYRIGHT`
- [x] `docs/ARCHITECTURE.md`

---

## Phase 1 — Infrastructure ✓

> CI green on all targets; release pipeline wired.

- [x] GitHub Actions on `aarch64-apple-darwin`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`
- [x] prek hooks for lints, tests, docs, and dependency policy, run from CI
- [x] `cargo-dist` skeleton

---

## Phase 2 — Core playback types ✓

> `aeri-core` defines what playback needs, and nothing more.

Small enough to fold into the start of Phase 3. Both phases are specified step by step in [`design/phase-2-3-playback.md`](design/phase-2-3-playback.md).

- [x] `SampleRate`, `Channels`, `StreamSpec`
- [x] Minimal `Track`: source, duration, optional title / artist / album ([ADR 0009](adr/0009-open-tag-metadata.md))
- [x] `PlaybackState`: stopped, or active with transport, position, and output spec
- [x] Unit tests for the type invariants

---

## Phase 3 — First audio

> `aeri play file.flac` produces sound. The decoder, engine, and sink are proven to compose.

- [x] `OutputSink` trait and `FakeSink` in `aeri-audio`
- [x] `aeri-decode`: `Decoder` over symphonia; FLAC and MP3 ([ADR 0007](adr/0007-symphonia-flac-mp3-first.md))
- [x] `aeri-audio`: `CoreAudioSink`, the real-time thread, `rtrb` ring buffer, atomics for paused and flush ([ADR 0003](adr/0003-native-output-backends.md))
- [ ] `aeri-audio`: `PipeWireSink`; report the negotiated rate
- [ ] `aeri-engine`: the playback actor ([ADR 0004](adr/0004-engine-is-an-actor.md)) with a current track and a next slot ([ADR 0005](adr/0005-engine-plays-current-and-next.md))
- [ ] `aeri-cli`: `play <path>` in the foreground
- [ ] Integration tests through `FakeSink`: sample counts, pause, seek, gapless and re-init transitions

---

## Phase 4 — Playback

> A player, not a file opener: playlists, a queue, playback order, volume, ReplayGain. Still driven from the CLI.

- [ ] Create `aeri-player`: the `Player` handle ([ADR 0006](adr/0006-frontends-use-the-player-handle.md)); move `aeri-cli` onto it and close the `deny.toml` exception
- [ ] Playlists with stable entry ids; the active playlist
- [ ] Playback order: default, repeat track, repeat playlist, shuffle
- [ ] The queue; stop after current
- [ ] The engine's "took the next track" event, and the player refilling the slot
- [ ] Create `aeri-dsp`: `DspNode`, the chain, a registry ([ADR 0002](adr/0002-compiled-in-extension-points.md))
- [ ] Volume and ReplayGain nodes; `rubato` resampler where PipeWire refuses a rate (CoreAudio converts on macOS, ADR 0017)
- [ ] `aeri-cli`: `play <paths…>`, directories, M3U files; interactive transport keys
- [ ] Gapless verified end to end across a playlist

---

## Phase 5 — Library

> Point aeri at a music directory; it remembers what is there.

- [ ] Metadata design: the `Track` type, tag-name normalisation, SQLite schema ([ADR 0009](adr/0009-open-tag-metadata.md))
- [ ] Create `aeri-library`: `rusqlite`, one writer thread, PRAGMAs ([ADR 0008](adr/0008-library-sqlite-and-in-memory-store.md))
- [ ] Scan pipeline: `jwalk` walk → extension filter → `lofty` read → batched upsert
- [ ] In-memory snapshot, published on a watch channel; string interning
- [ ] Measure at 100,000 generated tracks: memory, start-up load, full scan
- [ ] `notify` watcher for incremental rescans
- [ ] Playlists, the queue, and playback position persisted across restarts
- [ ] M3U / M3U8 import and export
- [ ] Fixtures covering edge-case tag encodings; scan idempotency tests

---

## Phase 6 — Scripting

> One language formats every string and answers every query.

- [ ] Compatibility decision: supported functions and operators, deliberate differences from foobar2000
- [ ] Create `aeri-script`: lexer, parser, tree ([ADR 0010](adr/0010-one-script-language.md))
- [ ] Title formatting: fields, functions, conditional sections
- [ ] Queries: comparison, text match, boolean operators, sort clauses
- [ ] Property tests and a fuzz target; evaluator benchmark at 100,000 tracks
- [ ] Library search, filters, and sorting through queries
- [ ] Autoplaylists
- [ ] `aeri-cli`: scripted output format; `search <query>`

---

## Phase 7 — First GUI

> Usable by the developer every day.

- [ ] GPUI verification before any screen: 50,000-row scripted table, deserialised splitter tree, accessibility, Linux requirements ([ADR 0011](adr/0011-gpui-and-layout-as-data.md))
- [ ] Create `aeri-gui`: application shell, widget trait and registry, layout tree loaded from a file
- [ ] Playlist view: scripted columns, grouping, sorting, selection, drag to reorder
- [ ] Playlist tabs
- [ ] Transport controls, seekbar, volume
- [ ] Library tree and filter widgets; search box
- [ ] Cover art; status bar
- [ ] Keyboard shortcuts
- [ ] Visual language decision: typography, spacing, colour

---

## Phase 8 — Platform integration (v0.1 milestone)

> A signed binary that behaves like a native player.

- [ ] Create `aeri-platform` as a `Player` frontend (ADR 0006)
  - Linux: MPRIS via `zbus`
  - macOS: Now Playing and remote commands via `objc2`
- [ ] Media keys; sleep inhibition during playback
- [ ] macOS: signed and notarized `.dmg` via `cargo-dist`
- [ ] Linux: AUR `PKGBUILD`

---

## v1.0 — The customisable player

> What makes it foobar: the user shapes it.

- [ ] Layout editing inside the application; layout presets
- [ ] Settings interface
- [ ] Tag editor
- [ ] DSP chain editor; equaliser
- [ ] ReplayGain scanner
- [ ] Fades on pause, stop, and seek; crossfade
- [ ] Queue viewer; playback statistics (play count, rating)
- [ ] Device selection
- [ ] Full keyboard navigation; accessibility audit
- [ ] Homebrew tap and AUR publish; Sigstore signing and provenance attestations

---

## Later

Unordered, and each needs its own design.

- More formats (ADR 0007)
- CUE sheets, including embedded
- Waveform seekbar, spectrum, VU meter
- Lyrics
- Scrobbling
- Internet radio and remote streams
- File operations: rename, move, copy by script
- Audio conversion
- Controlling a running instance from the CLI
- Third-party extensions (ADR 0002)

## Not planned

Windows. PulseAudio, ALSA, and JACK outputs. Audio CD playback and ripping. Tracker and video-game music formats. A daemon, a network API, streaming to other devices, a mobile client (ADR 0001).
