# Design brief — Phase 4: playback

This is the specification for taking aeri from `aeri play file.flac` to a player: playlists, a queue, playback order, volume, and ReplayGain, still driven from the command line. It is written to be executed step by step. Where it gives a type or trait, build that shape; where code and brief disagree after you have tried it, stop and raise it (see "When to stop and ask").

Read first: [`ARCHITECTURE.md`](../ARCHITECTURE.md) › Playback, [`RUST_STYLE.md`](../RUST_STYLE.md), [`TESTING.md`](../TESTING.md), ADRs [0002](../adr/0002-compiled-in-extension-points.md),
[0004](../adr/0004-engine-is-an-actor.md),
[0005](../adr/0005-engine-plays-current-and-next.md),
[0006](../adr/0006-frontends-use-the-player-handle.md),
[0017](../adr/0017-bit-perfect-without-hog-mode.md),
[0019](../adr/0019-pipewire-stream-on-its-own-loop.md).

## Outcome

```bash
cargo run -p aeri-cli -- play --order shuffle ~/Music/album-a ~/Music/mix.m3u song.flac
```

plays every FLAC and MP3 file it was given, in the chosen order, gaplessly where the stream spec allows, at ReplayGain-adjusted level. Keys pause, skip, seek, change the volume and the order, and quit. A file that cannot be played is reported and skipped. `aeri-cli` depends on `aeri-player` and `aeri-core` only.

Every step lands with its tests, and `prek run --all-files` plus `prek run --all-files --stage pre-push` stay green.

## Rules for this work

- **Signatures here are shapes, not gospel.** None of the code in this brief has been compiled. If the compiler or clippy objects, fix the signature in the idiomatic direction and keep the intent.
- **Verify every external API before using it.** Read the docs or source for the version in `Cargo.lock`, or the one `cargo add` resolves. What was checked while writing this brief is listed under "What this brief verified".
- **Add each dependency with the first line of code that uses it.** The only new external crates this brief names are in decision D4. Anything else is a question for the owner.
- **ADR before code.** Steps 2, 4, 5, and 6 each need an ADR ([`adr/README.md`](../adr/README.md)): two new crates, a new thread, a new channel, and a changed public trait.
- **One step per commit**, on the `phase-4` branch.

## Decisions

Agreed with the owner on 2026-11-16. The steps below build on them. Where a step says "ADR first", the ADR records the decision before the code is written.

| | Question | Decision | Applies in |
| --- | --- | --- | --- |
| D1 | The roadmap's `rubato` resampler "where PipeWire refuses a rate" | **Leave it out of Phase 4.** Neither sink ever refuses a rate: `CoreAudioSink` and `PipeWireSink` both report the requested spec as negotiated, because CoreAudio's output unit and the PipeWire server convert (ADR 0017, ADR 0019). A resampler built now would have no caller and no test that reaches it. The roadmap lists it under "Later". `rubato` stays the locked choice for when a caller exists. | No step |
| D2 | DSP buffer layout (an open decision in `ARCHITECTURE.md`) | **Interleaved.** The decoder and the sink are interleaved on both sides, and the two nodes of this phase are per-sample gains that do not care. Planar would cost two conversions per block to benefit no node that exists. A node that wants planar (an FFT equaliser) can convert inside itself. | Step 2 |
| D3 | Where ReplayGain values come from, with no tag reader until Phase 5 | **The decoder reports them.** symphonia already parses the tags when it probes a file, so `Decoder` gains one method and no dependency is added. This changes a public trait that crosses a crate boundary. The alternative, adding `lofty` a phase early, brings the tag-mapping question of ADR 0009 forward with it. | Step 4 |
| D4 | New dependencies | `fastrand` in `aeri-player`, for shuffle: seedable, no dependencies of its own, and shuffle needs no cryptographic quality. `crossterm` in `aeri-cli`, for raw-mode key input. A benchmark harness in `aeri-dsp` as a dev-dependency (D5). | Steps 3, 6, 9 |
| D5 | Benchmarks in CI under CodSpeed ([`TESTING.md`](../TESTING.md) › Not yet in place) | Agreed: a benchmark harness, a CI job, and CodSpeed. Linking a CodSpeed account to the GitHub repository is the owner's to do; ask on reaching Step 3, and until it is linked the benchmarks run locally only. | The CI half of Step 3 |
| D6 | Behaviour this brief chooses | ReplayGain defaults to album mode and falls back to the track value when a file has no album value, and the reverse in track mode. After queued tracks, the order resumes from the entry that was playing before the queue. Shuffle plays every entry once per pass, then reshuffles. `previous` always goes to the previous entry and never restarts the current one. All are pinned by tests and cheap to change. | Steps 1, 6 |

## Step 1 — `aeri-core`: volume and ReplayGain types

These cross `aeri-dsp`, `aeri-decode`, `aeri-engine`, and `aeri-player`, so they live in the crate all four depend on.

```rust
/// Linear output gain, from silence to unity. Never above unity, never NaN.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Volume(f32);

impl Volume {
    pub const SILENT: Self;
    pub const UNITY: Self;
    pub fn from_linear(gain: f32) -> Option<Self>;
    /// Decibels of attenuation; values above 0 dB clamp to unity.
    pub fn from_db(db: f32) -> Self;
    pub fn linear(self) -> f32;
    pub fn db(self) -> f32;
}

/// The ReplayGain values a file carries. Any of them may be missing.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ReplayGain {
    pub track_gain_db: Option<f32>,
    pub track_peak: Option<f32>,
    pub album_gain_db: Option<f32>,
    pub album_peak: Option<f32>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ReplayGainMode {
    Off,
    Track,
    #[default]
    Album,
}

impl ReplayGain {
    /// The linear factor to apply in `mode`, lowered so that `peak × factor ≤ 1`.
    /// `1.0` when the mode is off or the file carries no usable value.
    pub fn factor(&self, mode: ReplayGainMode) -> f32;
}
```

Idiom notes:

- **`Volume` derives `PartialEq` but not `Eq` or `Hash`.** Floats have NaN, which is not equal to itself, so `f32` is not `Eq`. The constructor rejects NaN, which would allow a hand-written `Eq`; nothing needs one yet, so do not write it.
- **The newtype is the validation.** `from_linear` is the only way in, so no node re-checks the range. A bare `f32` parameter would let `2.0` or NaN reach the audio path.
- **`factor` belongs on the type**, not in the DSP node: it is pure arithmetic with edge cases (missing values, a peak of zero, a non-finite gain from a malformed tag), and here it gets property tests and the 100% coverage gate.

Done when: unit and property tests cover the constructors and `factor`: the result is always finite and positive; off is exactly `1.0`; a missing value falls back as D6 says; `peak × factor ≤ 1` whenever a peak is present. No mutant survives in `aeri-core`.

## Step 2 — create `aeri-dsp`: `DspNode`, the chain, the registry

ADR first: the new crate, its place in the graph, and D2. Depends on `aeri-core` only. Add `{ crate = "aeri-dsp", wrappers = ["aeri-engine"] }` to `deny.toml`.

```rust
/// What a node is told before the first block of each track.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct TrackStart {
    pub spec: StreamSpec,
    pub replay_gain: ReplayGain,
}

/// A setting sent to every node in the chain. A node ignores what is not its own.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum Control {
    Volume(Volume),
    ReplayGain(ReplayGainMode),
}

pub trait DspNode: Send {
    /// A new track begins with the next block. Not on the hot path; may allocate.
    fn start_track(&mut self, start: &TrackStart);
    fn control(&mut self, control: &Control);
    /// Processes whole interleaved frames in place. Allocates nothing.
    fn process(&mut self, interleaved: &mut [f32]);
    /// Forgets history after a flush (seek, stop, skip).
    fn reset(&mut self);
}

pub struct Chain { /* Vec<Box<dyn DspNode>> */ }
pub struct Registry { /* id → constructor */ }
```

- **A node never changes the stream spec.** It processes in place, so frames in equals frames out. That keeps the gapless rule where it is: two tracks join gaplessly iff their decoders' specs are equal, whatever the chain holds. Anything that changes rate or channel count is a pipeline stage beside the chain, not a node.
- **`Chain`** fans each call out to its nodes in order. It splits `process` into pieces of at most 1024 frames, which is how the locked block size in `ARCHITECTURE.md` comes to hold; the engine hands over whatever the decoder produced.
- **`Registry`** maps a `&'static str` id to a `fn() -> Box<dyn DspNode>` (ADR 0002), and builds a `Chain` from a list of ids, failing on an unknown one. `Registry::builtin()` knows this crate's nodes. Nothing persists a chain in this phase, so the ids are not yet a compatibility surface.
- **`Control` is a closed enum, not a string-keyed parameter.** Every node is compiled in (ADR 0002), so the full set of settings is known at compile time and the compiler checks each use. A per-node settings blob for saving a chain waits for the settings-storage decision.

Idiom notes:

- **`Box<dyn DspNode>`** because a chain holds nodes of different types in one `Vec`. The cost is one indirect call per node per block, which is nothing beside the per-sample work.
- **`&mut [f32]` in place** borrows the engine's block for the length of the call. The borrow checker then guarantees no node keeps a reference to it afterwards.

Done when: `Chain` and `Registry` have unit tests using a test node, including a property test that any block length reaches the nodes as whole frames in pieces of at most 1024, in order, with nothing lost. Add the `aeri-dsp` 100% line to the `coverage` hook, as Step 1 of the previous brief did for `aeri-core`.

## Step 3 — `aeri-dsp`: the volume and ReplayGain nodes

- **`"replaygain"`** multiplies by `ReplayGain::factor` for the current track and mode. The factor changes only in `start_track` and `control`.
- **`"volume"`** multiplies by `Volume::linear`. A change is ramped linearly across the next piece it processes: a step in gain mid-signal is audible as a click.
- **At a factor of exactly `1.0` a node leaves the block untouched.** Multiplying by `1.0` is exact, so this is not for correctness; it makes "unity volume, ReplayGain off" a path that provably does not write the samples, which the bit-perfect claims of ADR 0017 and ADR 0019 lean on.
- `Registry::builtin()` order is ReplayGain, then volume.

Benchmarks arrive here ([`TESTING.md`](../TESTING.md)): one per node and one for the two-node chain, over a 1024-frame stereo block, built with the `profiling` profile. The CI job waits for the owner to link CodSpeed (D5).

Done when: unit tests pin the arithmetic; property tests show output never exceeds input magnitude at or below unity, and that a ramp is monotonic and ends on the target; the `process` bodies run under an allocation guard. The guard of ADR 0018 lives in `aeri-audio`, which `aeri-dsp` may not depend on, so say in the Step 2 ADR how `aeri-dsp` gets one. No mutant survives in `aeri-dsp`.

## Step 4 — `aeri-decode`: ReplayGain values, extensions, gapless trimming

ADR first (D3): `Decoder` gains

```rust
/// The ReplayGain values the file's tags carry.
fn replay_gain(&self) -> ReplayGain;
```

- Read the values once, at `open`, from the tags symphonia parsed. A value that does not parse is dropped with a `tracing::warn!`, not an error: a bad tag must not make a file unplayable.
- Export `EXTENSIONS: &[&str]`, the file extensions `open` can handle. It is re-exported upward in Steps 5 and 7 so the CLI can filter a directory without naming this crate.
- Confirm that symphonia trims MP3 encoder delay and padding. If it does, tighten the MP3 fixture test from "within codec padding of 44 100" to the exact count. If it does not, two MP3 files cannot join gaplessly; record that in the README and raise it.

Fixtures: add FLAC and MP3 files carrying known ReplayGain tags, and one with a malformed value, with their generating commands in the fixtures `README.md`. Extend the existing fuzz target to call `replay_gain`.

Done when: fixture tests read the known values from both formats and survive the malformed one.

## Step 5 — `aeri-engine`: the chain, the controls, the event

ADR first: the event channel (a new channel between threads), and the engine's dependency on `aeri-dsp`.

```rust
/// Something that happened in the engine and must not be missed (ADR 0004).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum EngineEvent {
    /// The engine took the next slot and is decoding this track. The slot is empty.
    TookNext(Arc<Track>),
    /// This track is now the one being heard.
    NowPlaying(Arc<Track>),
    /// A track could not be opened or failed while decoding, and was skipped.
    Failed(Arc<Track>),
    /// The current track ended with the slot empty; playback has stopped.
    Ended,
}

impl Engine {
    pub fn spawn<E>(
        sink: Box<dyn OutputSink>,
        events: mpsc::UnboundedSender<E>,
    ) -> Result<Self, EngineError>
    where
        E: From<EngineEvent> + Send + 'static;

    pub async fn set_volume(&self, volume: Volume) -> Result<(), EngineError>;
    pub async fn set_replay_gain(&self, mode: ReplayGainMode) -> Result<(), EngineError>;
}
```

- **The pipeline** becomes: decode a block → copy it into the engine's buffer (it already does) → `chain.process` in place → `sink.write`. Call `chain.start_track` with the decoder's spec and `replay_gain()` before the first block of every track, including a gapless one, so the gain changes on the exact frame the track does. Call `chain.reset` wherever the sink is flushed.
- **Two events, because there are two moments.** `TookNext` fires when the decoder of the current track is exhausted, up to a ring's length (200 ms, ADR 0016) before the change is heard; the owner needs it then, to refill the slot in time. `NowPlaying` fires when `frames_played` reaches the new track, which is when a display should change. The engine already tracks that moment for its published state.
- **The sender is unbounded** so the playback thread never waits on a slow receiver. Events are a few per track.
- **Late changes to the slot** (the cut-off ADR 0005 left to this phase): once `TookNext` has fired, that track plays. A `set_next` after it fills the slot for the track after. Pin it with a test.
- **Re-export `aeri_decode::EXTENSIONS`**, so the crates above can ask what is playable.
- **A volume change is heard after the audio already in the ring and the device buffer**, about a quarter of a second. `ARCHITECTURE.md` accepts that latency; check by hand in Step 10 that it is tolerable on a held key.
- **`test-util` feature:** re-export `FakeSink` and its observer from `aeri-audio`. `aeri-player`'s tests must use the real engine over `FakeSink` (ADR 0006) and `deny.toml` forbids them from naming `aeri-audio`.

Idiom note: **`E: From<EngineEvent>`** lets the owner pass the sender of its *own* inbox. The player has one channel carrying `enum Message { Command(..), Engine(EngineEvent) }` and implements `From<EngineEvent> for Message`, so its thread waits on one receiver with `blocking_recv`, with no runtime and no `select!`. The engine's tests pass a plain `UnboundedSender<EngineEvent>`, which works because every type is `From` itself. Only `spawn` is generic; `Engine` stays a plain type.

Done when these pass against `FakeSink`, alongside the existing nine:

1. `play`, `set_next` same spec → `TookNext` then `NowPlaying` for the second track, in that order, then `Ended`.
2. `set_next` to a missing file → `Failed`, then `Ended` after the current track.
3. `set_next` after `TookNext` → the taken track plays in full; the new one follows.
4. Volume at half → every frame the sink received is half the decoded value, after the ramp.
5. Two same-spec tracks with different track gains, track mode → the gain changes on the first frame of the second track, with one `configure`.
6. Unity volume, ReplayGain off → the sink received exactly the decoded samples.
7. Seek → the chain was reset.

## Step 6 — `aeri-player`: playlists, order, and the queue as pure logic

ADR first: the new crate and its thread (Step 7). Depends on `aeri-core` and `aeri-engine`. This step writes no thread: everything that decides what plays next is plain data and functions, so it can be property-tested without an engine.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PlaylistId(NonZeroU64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntryId(NonZeroU64);

#[derive(Debug, Clone)]
pub struct Entry {
    pub id: EntryId,
    pub track: Arc<Track>,
}

/// An ordered list of entries. The same track may appear more than once.
#[derive(Debug, Clone)]
pub struct Playlist { /* id, name, Vec<Entry> */ }

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PlaybackOrder {
    #[default]
    Default,
    RepeatTrack,
    RepeatPlaylist,
    Shuffle,
}
```

- **Ids are allocated from a counter and never reused.** An index names a position, and positions move under every edit; an id names an entry. Every command that names an entry takes an id, and a command naming an id that no longer exists is an error, not a panic.
- **`Playlist`** supports insert at a position, remove by id, move by id, and clear. It never hands out indices that outlive the call.
- **One function answers "what plays next"** from the playlists, the active playlist, the cursor, the order, the shuffle state, the queue, and stop-after-current. The actor of Step 7 calls it and nothing else decides. The rules, in priority order:
  1. stop-after-current set → nothing;
  2. the queue is not empty → its front;
  3. otherwise the order's choice in the active playlist, relative to the cursor.
- **The queue** is a list of `(PlaylistId, EntryId)`. Removing an entry from its playlist removes it from the queue.
- **Shuffle** keeps a permutation of the active playlist's entry ids and a place in it. An entry added mid-pass joins the unplayed part at a random place; a removed one is dropped. The random source is a `fastrand::Rng` owned by the shuffle state and seeded by the caller, so tests are deterministic.
- **Stop-after-current** clears itself when it takes effect.

Idiom notes:

- **`NonZeroU64` inside the id** gives `Option<EntryId>` the same size as `EntryId`: the compiler uses zero for `None`.
- **Pure functions over owned data** are what make this testable. The actor adds ordering of messages and nothing else.

Done when: property tests drive arbitrary sequences of edits and "advance" calls and hold these invariants. Ids stay unique. The next entry always exists in the playlist. Default order visits each entry once and ends. Repeat-playlist never ends on a non-empty playlist. A shuffle pass visits every entry exactly once, whatever edits happen mid-pass. A queued entry plays before any other. Commit the `proptest-regressions`. No mutant survives.

## Step 7 — `aeri-player`: the `Player` handle

The actor (ADR 0004, ADR 0006): one named thread owning the playlists, the queue, the order, and the engine handle; one inbox for commands and engine events.

```rust
#[derive(Debug, Clone)]
pub struct Player { /* command sender + state receivers */ }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seek {
    To(Duration),
    Forward(Duration),
    Back(Duration),
}

/// Everything a frontend renders except playback position.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PlayerState {
    pub playlists: Arc<[Arc<Playlist>]>,
    pub active: Option<PlaylistId>,
    /// The entry being heard.
    pub current: Option<(PlaylistId, EntryId)>,
    pub queue: Arc<[(PlaylistId, EntryId)]>,
    pub order: PlaybackOrder,
    pub stop_after_current: bool,
    pub volume: Volume,
    pub replay_gain: ReplayGainMode,
}

impl Player {
    pub fn with_default_output() -> Result<Self, PlayerError>;

    pub async fn create_playlist(&self, name: String) -> Result<PlaylistId, PlayerError>;
    pub async fn append(&self, to: PlaylistId, tracks: Vec<Arc<Track>>) -> Result<Vec<EntryId>, PlayerError>;
    pub async fn remove(&self, from: PlaylistId, entries: Vec<EntryId>) -> Result<(), PlayerError>;
    pub async fn move_before(&self, playlist: PlaylistId, entry: EntryId, before: Option<EntryId>) -> Result<(), PlayerError>;
    pub async fn set_active(&self, playlist: PlaylistId) -> Result<(), PlayerError>;

    pub async fn play(&self) -> Result<(), PlayerError>;
    pub async fn play_entry(&self, playlist: PlaylistId, entry: EntryId) -> Result<(), PlayerError>;
    pub async fn toggle_pause(&self) -> Result<(), PlayerError>;
    pub async fn stop(&self) -> Result<(), PlayerError>;
    pub async fn next(&self) -> Result<(), PlayerError>;
    pub async fn previous(&self) -> Result<(), PlayerError>;
    pub async fn seek(&self, seek: Seek) -> Result<(), PlayerError>;

    pub async fn enqueue(&self, playlist: PlaylistId, entry: EntryId) -> Result<(), PlayerError>;
    pub async fn set_order(&self, order: PlaybackOrder) -> Result<(), PlayerError>;
    pub async fn set_stop_after_current(&self, stop: bool) -> Result<(), PlayerError>;
    pub async fn set_volume(&self, volume: Volume) -> Result<(), PlayerError>;
    pub async fn set_replay_gain(&self, mode: ReplayGainMode) -> Result<(), PlayerError>;

    pub fn subscribe(&self) -> watch::Receiver<PlayerState>;
    /// The engine's own state, passed through: position, transport, output spec.
    pub fn playback(&self) -> watch::Receiver<PlaybackState>;
}
```

- **Two watch channels, not one.** Position changes ten times a second; playlists change when someone edits them. Passing the engine's `PlaybackState` receiver through means the player thread never wakes for a position tick, and a frontend redraws its playlist only when the playlist changed.
- **Refilling the slot.** On `TookNext`, move the cursor to that entry, ask the Step 6 function for the following one, and call `set_next`. Do the same after any command that changes the answer: an edit, a queue change, an order change, stop-after-current. Compare with what the slot already holds and skip the call when it is the same track.
- **`current` follows `NowPlaying`**, not `TookNext`, so the displayed entry changes when the sound does. Between the two events the player holds the taken entries in order, first in, first out. That also keeps repeat-track and a file listed twice unambiguous without comparing tracks.
- **Failures skip, with a bound.** When the engine reports `Failed`, or `play` returns a decode error, move on to the next entry by the same rules. Stop after as many consecutive failures as the active playlist has entries: repeat-playlist over files that have all been deleted must not spin forever.
- **`toggle_pause` and relative `Seek` are resolved by the actor** (ADR 0004): a frontend that read "playing" a moment ago and sent "pause" would race another frontend doing the same.
- **Snapshots are shared, not copied.** `PlayerState` holds `Arc`s; an edit builds a new `Arc<Playlist>` and publishes. That copy is linear in the playlist's length per edit. Accepted for this phase; Phase 5 measures at 100,000 tracks.
- **Shutdown** follows the engine: dropping the last `Player` closes the inbox, the thread drops the engine, and both threads join.
- **`deny.toml`:** `aeri-engine`'s wrapper becomes `aeri-player`. Move `aeri-cli` onto `Player` in this commit, still playing one path, so the exception closes here and the tree stays green. Update the dependency rules in `ARCHITECTURE.md` and `AGENTS.md` § 2.
- **Test construction:** behind a `test-util` feature, a constructor that builds the player over the engine's `FakeSink` with a given shuffle seed.

Idiom note: **`Seek` is an enum** because `std::time::Duration` cannot be negative. Three variants say what a signed number and a flag would, and the compiler makes every caller handle each.

Done when these pass against `FakeSink`:

1. Three same-spec tracks, default order → one `configure`, frames equal to the sum of all three, `current` visits each entry in order, then stopped. This is the roadmap's "gapless verified end to end across a playlist".
2. Each playback order produces its sequence, with a fixed shuffle seed.
3. `enqueue` during the first track → the queued entry plays second, then the order resumes (D6).
4. Stop-after-current → playback stops at the end of the track and the flag clears.
5. Removing the entry that is in the slot → the slot is refilled with the one after it.
6. `next` and `previous` at both ends of the playlist, in each order.
7. A missing file between two good ones → skipped; an all-missing playlist under repeat-playlist → stops.
8. `toggle_pause` twice, and each `Seek` variant, including past either end.
9. Dropping every `Player` joins both threads, under a timeout.

## Step 8 — `aeri-cli`: paths, directories, M3U

- `play <paths…>` takes one or more paths. A file is added; a directory is walked recursively with `fs_err`, sorted by path, keeping files whose extension the player reports as playable; an `.m3u` or `.m3u8` file is parsed.
- **The M3U parser** lives in `aeri-player` as a pure function from text and a base directory to a list of `Source`s: one path per line, `#` lines ignored, relative paths resolved against the playlist's directory, a UTF-8 byte-order mark and CRLF line endings accepted. It does no I/O and never panics. Phase 5's import and export build on it.
- Flags: `--order`, `--volume` (in dB), `--replaygain`. `clap`'s `ValueEnum` derive maps the enum variants to their names.
- A path that does not exist is an error before the audio device opens, as today. An empty result (a directory with no playable file) is an error too.
- Tracks carry no tags in this phase: the display shows the file name. Titles arrive with the library.

Done when: the parser has fixture tests, property tests, and a fuzz target in `fuzz/` ([`TESTING.md`](../TESTING.md): anything parsing bytes we did not write); directory expansion has tests over a temporary tree; the binary's integration tests cover the two error cases.

## Step 9 — `aeri-cli`: transport keys

- With `crossterm`, put the terminal in raw mode and read keys on a named thread that forwards them over a channel to the `select!` loop. Verify the crate's features and API for the resolved version first.
- Keys: space toggles pause; `n` and `p` skip; left and right seek by five seconds; up and down change the volume by 2 dB; `o` cycles the order; `s` toggles stop-after-current; `q` quits.
- **Raw mode delivers Ctrl-C as a key, not a signal.** Handle it as `q`, and keep the signal handler for the case below.
- **When stdin is not a terminal** (`std::io::IsTerminal`), skip raw mode and keys entirely. The integration tests and `aeri play … < /dev/null` must still work.
- **Restore the terminal on every exit path.** Hold raw mode in a guard whose `Drop` restores it, so an early `?` return and a panic both leave a usable shell.
- The progress line gains the order, the volume, and "n of m".

Idiom note: **a guard type with `Drop`** is Rust's `try`/`finally`. The compiler runs `drop` when the value goes out of scope however the scope is left, so there is no path that forgets.

Done when: key-to-command mapping is a pure function with unit tests; the output module's new lines have tests on their exact text; the binary's test with stdin not a terminal still passes. Everything else in this step is checked by hand.

## Step 10 — verify by listening

Only the owner can do this, on both platforms. Record the result in the README's Status and the roadmap.

- A gapless album in FLAC, and one in MP3: no gap or click at any boundary.
- Tracks at different sample rates in one playlist: the re-init gap, and nothing worse.
- Volume up and down on a held key: no clicks, and the delay is tolerable.
- ReplayGain in track and album modes against files with known values.
- Skip, seek, and pause under rapid key presses.

## What this brief verified

- `symphonia-core` 0.6.1 (the version in `Cargo.lock`), read in the local cargo registry on 2026-11-16: `StandardTag` has `ReplayGainTrackGain`, `ReplayGainTrackPeak`, `ReplayGainAlbumGain`, and `ReplayGainAlbumPeak`, each carrying a string; the audio decoder options have a `gapless` flag, on by default, documented as trimming delay and padding frames.
- The bridge's ring holds 200 ms (`crates/aeri-audio/src/bridge.rs`).
- Both device sinks report the requested spec as negotiated (ADR 0017, ADR 0019, and `Playback::start`'s warning path, which therefore never runs on a real device).

Not verified: that `aeri-decode` leaves `gapless` on and that MP3 trimming is sample-exact; which tag formats symphonia maps to those `StandardTag` variants for MP3 in practice; anything about `fastrand`, `crossterm`, or a benchmark harness, including their licences, features, and build scripts against `deny.toml`; and every signature above.

## When to stop and ask

Stop and raise it with the owner, do not improvise, when:

- a signature here cannot be made to compile without changing its *meaning*;
- you want a dependency this brief does not name, or one it names fails `cargo deny`;
- a decision in the table above looks wrong once real code exists;
- an existing `Engine` method, `OutputSink`, or `Decoder` needs to change beyond what Steps 4 and 5 specify;
- a decision recorded in an ADR looks wrong once real code exists;
- a check fails and the only fix you can see is to weaken the check.

## Not in this phase

A resampler (D1). Tags other than ReplayGain, and anything shown from them. Persisting playlists, the queue, settings, or position. A queue command in the CLI: the queue is reached through `Player` and its tests until a frontend can point at an entry. A "viewed" playlist distinct from the active one, which is GUI state. Saving or editing a DSP chain, an equaliser, fades, and crossfade. Device selection. Controlling a running instance from another shell. If a step seems to need one of these, the step is being over-built.
