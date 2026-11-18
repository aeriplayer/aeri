# Design brief — Phase 5: library

This is the specification for taking aeri from a player that is handed paths to one that knows a music directory: it scans it, reads every tag, remembers what it found across restarts, notices when files change, and restores playlists, the queue, and playback position. It is written to be executed step by step. Where it gives a type or trait, build that shape; where code and brief disagree after you have tried it, stop and raise it (see "When to stop and ask").

Read first: [`ARCHITECTURE.md`](../ARCHITECTURE.md) › Library, [`RUST_STYLE.md`](../RUST_STYLE.md), [`TESTING.md`](../TESTING.md), ADRs
[0004](../adr/0004-engine-is-an-actor.md),
[0006](../adr/0006-frontends-use-the-player-handle.md),
[0008](../adr/0008-library-sqlite-and-in-memory-store.md),
[0009](../adr/0009-open-tag-metadata.md),
[0022](../adr/0022-decoder-reports-replaygain.md),
[0025](../adr/0025-player-actor-and-offers.md),
[0030](../adr/0030-aeri-changes-nothing-it-was-not-told-to.md).

## Outcome

```bash
cargo run -p aeri-cli -- scan ~/Music
cargo run -p aeri-cli -- play
```

The first reads every FLAC and MP3 file under `~/Music` into the library and reports how many it added, changed, and could not read; run again with nothing changed, it writes nothing. The second, with no paths, restores the playlists, the queue, the playback order, and the position from the last run, and shows titles and artists from the tags. `play <paths…>` works as before, now with tags. No file aeri reads is ever written (ADR 0030).

Every step lands with its tests, and `prek run --all-files` plus `prek run --all-files --stage pre-push` stay green.

## Rules for this work

- **Signatures here are shapes, not gospel.** None of the code in this brief has been compiled. If the compiler or clippy objects, fix the signature in the idiomatic direction and keep the intent.
- **Verify every external API before using it.** Read the docs or source for the version in `Cargo.lock`, or the one `cargo add` resolves. What was checked while writing this brief is listed under "What this brief verified"; it is far less than Phase 4's, because none of these crates is in the lockfile yet.
- **Add each dependency with the first line of code that uses it.** The only new external crates this brief names are in decision D5. Anything else is a question for the owner.
- **ADR before code.** Steps 1, 3, 7, and 8 each need an ADR ([`adr/README.md`](../adr/README.md)): the metadata schema, a new crate with a new thread, a new dependency under a licence the allowlist does not hold, and a change to the `Player` handle.
- **One step per commit**, on the `phase-5` branch.

## Decisions

Agreed with the owner on 2026-11-17. The steps below build on them. Where a step says "ADR first", the ADR records the decision before the code is written.

| | Question | Decision | Applies in |
| --- | --- | --- | --- |
| D1 | The shape of `Track` (ADR 0009) | **`aeri-core` keeps `Track`**, now a source, typed audio properties, and an open tag map. The engine, the player, and every frontend pass tracks, so the type stays in the crate all of them share; only reading tags from files lives in `aeri-library`. The map is a sorted boxed slice of `(FieldName, values)`, not a `HashMap`: a track carries a dozen or two fields, where a binary search over a slice beats hashing and costs a fraction of the memory. | Steps 1, 2 |
| D2 | Field-name normalisation (ADR 0009's compatibility surface) | **foobar2000's field names, stored as ASCII upper case.** Each format's keys (ID3v2 frames, Vorbis comments) map to one name through `lofty`'s format-independent keys, plus a table of our own for what it does not cover. A key aeri has no mapping for keeps its native name, upper-cased, so `%mood%` works on a field aeri has never heard of. Non-ASCII letters in a name are left as they are. | Steps 1, 4 |
| D3 | The SQLite schema and how it changes | **The schema in Step 1, versioned with `PRAGMA user_version`** and a list of migration functions applied in order at open. No migration crate: the list is a dozen lines and the version is one integer SQLite already stores. Paths are stored as raw bytes, because a Linux path need not be UTF-8. | Steps 1, 3 |
| D4 | What a track's identity is | **Its path.** A file is unchanged when its size and modification time match the stored ones, and is not re-read. A file that disappears is marked missing, never deleted: playlists keep their entries, and a file that comes back (a disk remounted) is found again. Moves are a missing file plus a new one; recognising a move by content is curation work for later. | Steps 1, 5 |
| D5 | New dependencies | `rusqlite` with `bundled` (ADR 0008), `lofty` without its default `id3v2_compression_support` unless a fixture needs it, `jwalk` and `rayon` for the walk and the reads, `notify` for watching (D6). **No** interning crate: an interner is a `HashSet<Arc<str>>` the scanner owns, a dozen lines. **No** debouncer crate: the watcher coalesces events itself (Step 7). **No** platform-directories crate (D7). Each is checked against `deny.toml`, the seven-day cooldown, and its build script when it is added. | Steps 3–7 |
| D6 | `notify` is `CC0-1.0`, and on Linux it pulls `inotify` and `inotify-sys`, which are `ISC` | **Grant three per-crate licence exceptions, recorded in an ADR**, the way ADR 0014 did for symphonia. Neither licence is in the allowlist; both are permissive. The alternatives are worse: watching without `notify` means FSEvents and inotify FFI, which is `unsafe` outside `aeri-audio`; leaving watching out of the phase leaves the library stale until the next manual scan. This relaxes a `deny.toml` rule; the owner agreed to these three exceptions and no others. | Step 7 |
| D7 | Where the database lives | **The binary decides; the library takes a path.** `aeri-cli` resolves `~/Library/Application Support/aeri/library.db` on macOS and `$XDG_DATA_HOME/aeri/library.db` (falling back to `~/.local/share`) on Linux from environment variables, a dozen lines, and `--database <path>` overrides it. Tests pass a temporary path. The database is aeri's own derived data, never the user's files (ADR 0030). | Steps 3, 10 |
| D8 | `Player::with_default_output()` takes no arguments, and a player with a library needs a database path | **Replace it with `Player::open(PlayerConfig)`**, where the config names the database and the output. This changes an existing method of the `Player` handle (AGENTS.md rule 8), as does Step 8's change to `append`; the owner agreed to both. The alternative, keeping `with_default_output` and adding a second constructor, leaves a player with no library that every frontend must know not to use. | Step 8 |
| D9 | What is persisted, and when | **Playlists, the queue, the active playlist, the current entry and its position, the order, the volume, and the ReplayGain mode.** Edits are written as they happen, as jobs to the library's writer. Position is written on pause, stop, track change, seek, and shutdown, not on every tick: a crash loses at most the position within the current track. Volume, order, and ReplayGain mode are stored as player state in the same table; the settings-storage decision in `ARCHITECTURE.md` stays open for real settings. | Step 8 |
| D10 | ReplayGain now has two readers: the decoder (ADR 0022) and `lofty` | **Playback keeps the decoder's values this phase.** The library stores the ReplayGain tags as ordinary fields, for display and queries. Moving playback onto the library's values would change the `Decoder` trait again for no gain a user can hear. The cost is two parsers that can disagree on a malformed tag; a fixture pins what each does with the Phase 4 malformed file. | Step 4 |
| D11 | The 100,000-track budget (ADR 0008) | **The snapshot under 300 MB resident, start-up load under one second, on the owner's Mac.** A full scan is measured and recorded, not budgeted: it depends on the disk and the page cache. If either budget fails, stop: ADR 0008 says to revisit, and its fallback is a cached binary snapshot. | Step 6 |

## Step 1 — the metadata ADR

Documentation only. One ADR records D1 to D4, implementing what ADR 0009 deferred to this phase (it is not superseded: its decision stands and this fills it in). Move "Metadata schema" from Open decisions to Resolved in `ARCHITECTURE.md`.

The ADR carries the field mapping table, starting from these foobar2000 names: `TITLE`, `ARTIST`, `ALBUM`, `ALBUM ARTIST`, `DATE`, `GENRE`, `TRACKNUMBER`, `TOTALTRACKS`, `DISCNUMBER`, `TOTALDISCS`, `DISCSUBTITLE`, `COMPOSER`, `PERFORMER`, `COMMENT`, and the four `REPLAYGAIN_*` fields. Check each against foobar2000's documentation and against `lofty`'s key list at the version `cargo add` resolves, and record both sources in the ADR.

The schema, as a shape:

```sql
CREATE TABLE track (
    id          INTEGER PRIMARY KEY,
    path        BLOB    NOT NULL UNIQUE,
    size        INTEGER NOT NULL,
    mtime_ns    INTEGER NOT NULL,
    missing     INTEGER NOT NULL DEFAULT 0,
    duration_ns INTEGER,
    sample_rate INTEGER,
    channels    INTEGER,
    bit_depth   INTEGER,
    bitrate     INTEGER,
    codec       TEXT
) STRICT;

CREATE TABLE tag (
    track_id INTEGER NOT NULL REFERENCES track (id) ON DELETE CASCADE,
    field    TEXT    NOT NULL,
    ordinal  INTEGER NOT NULL,
    value    TEXT    NOT NULL,
    PRIMARY KEY (track_id, field, ordinal)
) STRICT, WITHOUT ROWID;

CREATE TABLE root (id INTEGER PRIMARY KEY, path BLOB NOT NULL UNIQUE) STRICT;

CREATE TABLE playlist (
    id INTEGER PRIMARY KEY, name TEXT NOT NULL, position INTEGER NOT NULL
) STRICT;

CREATE TABLE playlist_entry (
    id          INTEGER PRIMARY KEY,
    playlist_id INTEGER NOT NULL REFERENCES playlist (id) ON DELETE CASCADE,
    position    INTEGER NOT NULL,
    track_id    INTEGER NOT NULL REFERENCES track (id)
) STRICT;

CREATE TABLE queue (
    position INTEGER PRIMARY KEY,
    entry_id INTEGER NOT NULL REFERENCES playlist_entry (id) ON DELETE CASCADE
) STRICT;

-- One row: the player's state across restarts (D9).
CREATE TABLE player_state (...) STRICT;
```

- **Every track aeri has read is a `track` row**, whether it came from a scan or from `play <path>`. A track is in the library when its path is under a `root`. That keeps one kind of row for a playlist entry to point at.
- **`playlist_entry.id` is the `EntryId`**, and `playlist.id` the `PlaylistId`, so ids survive a restart. The player's counters resume above the highest stored id: ids are never reused (Phase 4, Step 6).
- **`STRICT` tables** make SQLite reject a value of the wrong type instead of storing it. Check that the bundled SQLite version supports it (3.37 or later) when `rusqlite` is added.

Done when: the ADR records D1 to D4 as agreed, with the field mapping table checked against both sources.

## Step 2 — `aeri-core`: the new `Track`

```rust
/// A field name, normalised: ASCII upper case (ADR 00NN). Compared and ordered by that form.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FieldName(Arc<str>);

/// A track's tags: each field holds one or more values, in the order the file gave them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tags(Box<[(FieldName, Box<[Arc<str>]>)]>);

impl Tags {
    /// Every value of `name`, matched case-insensitively. Empty when the field is absent.
    pub fn get(&self, name: &str) -> &[Arc<str>];
    pub fn first(&self, name: &str) -> Option<&str>;
    pub fn iter(&self) -> impl Iterator<Item = (&FieldName, &[Arc<str>])>;
    pub fn title(&self) -> Option<&str>;
    pub fn artist(&self) -> Option<&str>;
    pub fn album(&self) -> Option<&str>;
    pub fn album_artist(&self) -> Option<&str>;
}

impl FromIterator<(FieldName, Arc<str>)> for Tags { /* sorts, groups, keeps value order */ }

/// What the stream says about itself, independent of tags. Any of it may be unknown.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct AudioProperties {
    pub duration: Option<Duration>,
    pub sample_rate: Option<SampleRate>,
    pub channels: Option<Channels>,
    pub bit_depth: Option<NonZeroU8>,
    pub bitrate_kbps: Option<NonZeroU32>,
    pub codec: Option<Codec>,
}

pub struct Track {
    pub source: Source,
    pub properties: AudioProperties,
    pub tags: Tags,
}
```

- **The typed accessors are views** over the map (ADR 0009): `title()` is `first("TITLE")`. There is no second store to keep in step.
- **`Codec`** is a `#[non_exhaustive]` enum of `Flac` and `Mp3`, the formats aeri decodes (ADR 0007).
- **Move every caller in this commit**: the engine, the player, the CLI, and their tests, so the tree stays green. The CLI's display reads `tags.title()` and falls back to the file name, as today.

Idiom notes:

- **`Arc<str>` rather than `String`** for names and values. A `String` owns a growable buffer; an `Arc<str>` is one immutable allocation shared by reference count, so a thousand tracks by one artist can point at one copy (the interner of Step 6), and cloning a track never copies text. It is `Arc` and not `Rc` because snapshots cross threads.
- **`Box<[T]>` rather than `Vec<T>`** for data that never grows after it is built: two words instead of three, and no spare capacity left over from building it. Build a `Vec`, then `into_boxed_slice()`.
- **`FromIterator`** is what lets callers write `.collect::<Tags>()`, and puts the sorting and grouping in one place that the property tests can reach.
- **`NonZeroU8` and `NonZeroU32`** say "zero is not a real value here" in the type, and make the `Option` around them free (Phase 4, Step 6).

Done when: unit and property tests show that lookup is case-insensitive, that any input order collects to the same `Tags`, that values of one field keep their input order, and that `get` on an absent field is empty. `aeri-core` keeps 100% line coverage and no mutant survives.

## Step 3 — create `aeri-library`: the database and its writer

ADR first: the new crate, its place in the graph, its thread, and its channel. Depends on `aeri-core` only among workspace crates (see the extension list below). Add `{ crate = "aeri-library", wrappers = ["aeri-player"] }` to `deny.toml`, and update the dependency rules in `ARCHITECTURE.md` and AGENTS.md § 2.

```rust
/// The library: one writer thread owning the SQLite connection (ADR 0008).
#[derive(Debug, Clone)]
pub struct Library { /* job sender + snapshot receiver */ }

impl Library {
    /// Opens or creates the database at `path`, applies the PRAGMAs, migrates, and loads the
    /// snapshot. Blocks until the snapshot is loaded.
    pub fn open(path: &Path) -> Result<Self, LibraryError>;
    pub fn snapshot(&self) -> watch::Receiver<Arc<Snapshot>>;
}
```

- **One connection, one named thread** (ADR 0008). Writes arrive as jobs on a channel; each job that changes tracks commits one transaction and publishes a new snapshot.
- **PRAGMAs** from `ARCHITECTURE.md`, applied and then read back in a test: `journal_mode` reports what it got, and a database on a filesystem without WAL support silently stays in rollback mode.
- **Migrations** (D3): read `user_version`, apply each migration above it inside one transaction, set the new version. A database from a *newer* aeri is an error, never a downgrade.
- **The extension list.** The scanner needs `aeri_decode::EXTENSIONS`, but `deny.toml` lets only `aeri-engine` depend on `aeri-decode`. Move the list into `aeri-core` as data (`Codec::extensions()`), have `aeri-decode` assert in a test that it opens exactly those, and remove the re-exports through the engine and the player. Raise it if that move looks wrong in the code.
- **`rusqlite` brings C** (bundled SQLite) and a build script. Add `libsqlite3-sys` to `allow-build-scripts`. Miri cannot run C; `aeri-library` has no `unsafe` and no floating-point arithmetic of its own, so it is not a Miri crate ([`TESTING.md`](../TESTING.md)).

Idiom note: **`watch::Receiver<Arc<Snapshot>>`** hands every reader the newest snapshot without copying it and without a lock. The writer builds a new `Snapshot` and sends the `Arc`; a reader that holds an old one keeps it alive until it lets go. This is ADR 0004's state channel, reused.

Done when: tests over a temporary database open, migrate from empty, reopen at the current version, refuse a newer version, and survive a job sent after the last handle is dropped. Dropping the last `Library` joins the thread, under a timeout.

## Step 4 — `aeri-library`: reading tags

A pure-ish function from a path to a `Track`, over `lofty`:

```rust
pub(crate) fn read_track(path: &Path, interner: &mut Interner) -> Result<Track, ReadError>;
```

- **Read only.** Use `lofty`'s read API and nothing that saves. ADR 0030 says to check, when a tag library is added, that it writes nothing unasked: confirm from `lofty`'s source that reading opens the file read-only, and record it in the Step 3 ADR.
- **Map keys by D2.** Several values for one field, whether as repeated Vorbis comments or ID3v2.4's separated values, become several values, in file order. Check how `lofty` presents each at the resolved version before writing the mapping.
- **A tag that cannot be decoded is dropped with a `tracing::warn!`**, not an error. A file whose tags are unreadable but whose audio is fine is still a track, with properties and no tags.
- **ReplayGain** fields are stored as text like any other (D10).

Fixtures, each with its generating command in the fixtures `README.md`: ID3v1 only; ID3v2.3 with UTF-16 text; ID3v2.4 with a multi-value artist; both ID3v1 and ID3v2 in one MP3, disagreeing; FLAC with repeated `ARTIST` comments and mixed-case keys; a custom field; an invalid UTF-8 value; a truncated tag; no tags at all. Add a fuzz target in `fuzz/` that feeds arbitrary bytes through `read_track` ([`TESTING.md`](../TESTING.md): anything parsing bytes we did not write).

Done when: each fixture reads to the expected `Tags`; the fuzz target runs; the Phase 4 malformed-ReplayGain file is pinned under both readers (D10).

## Step 5 — `aeri-library`: the scan

```rust
impl Library {
    /// Adds `root` to the library and scans it. Completes when the scan has been committed.
    pub async fn scan(&self, root: PathBuf) -> Result<ScanReport, LibraryError>;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ScanReport { pub added: u64, pub changed: u64, pub unchanged: u64, pub missing: u64, pub unreadable: u64 }
```

The pipeline, on its own named thread so the writer keeps serving jobs:

1. **Walk** the root with `jwalk`, following no symlinks out of the root, keeping files whose extension is in `Codec::extensions()`, case-insensitively.
2. **Compare** each file's size and modification time with the snapshot (D4). Unchanged files go no further.
3. **Read** the rest in parallel with `rayon`, through Step 4.
4. **Send** results to the writer in batches of a few hundred over a bounded channel, so a fast scanner waits for a slow disk instead of filling memory. The writer upserts each batch in one transaction and publishes a snapshot.
5. **Mark missing** every track under the root that the walk did not see, in the last batch.

- **Idempotent by construction**: a second scan with nothing changed reads no file and writes no row. A test counts the writes.
- **The walk and the reads touch nothing**: no access-time update is requested, no file is opened for writing (ADR 0030).
- A file that cannot be read is counted and logged, and keeps whatever row it had. Storing the reasons is for the library health report (Curation), not this phase.

Done when: tests over a temporary tree of fixtures cover add, change (rewrite a copy's tags, which aeri may do to files it created for the test), unchanged, missing, a file coming back, an unreadable file, and the second-scan-writes-nothing case.

## Step 6 — the snapshot, interning, and the measurement

```rust
/// Every track the library knows, immutable (ADR 0008).
#[derive(Debug)]
pub struct Snapshot { /* tracks: Box<[Arc<Track>]>, by_path: HashMap<…, usize> */ }
```

- **Interning.** One `Interner` (a `HashSet<Arc<str>>`) lives on the writer thread for the life of the library; every field name and value passes through it on the way into a `Track`, from a scan and from the start-up load alike. Equal strings become one allocation.
- **Start-up load** reads `track` and `tag` in two ordered queries and builds the snapshot without a query per track.
- **Publishing** builds a new `Box<[Arc<Track>]>` per batch: a copy of one pointer per track. Measure it at 100,000.

The measurement, as a benchmark in `aeri-library` (the harness from Phase 4, D5), over a generated database of 100,000 tracks with realistic tags: 2,000 artists, 8,000 albums, a dozen fields each. Generate rows, not audio files, for the load and memory numbers. For the scan, generate copies of one small fixture with varied tags into a temporary directory. Record the three numbers and the machine in the Step 3 ADR's consequences, and in [`TESTING.md`](../TESTING.md) › Benchmarks.

Done when: the numbers are recorded and within D11's budgets, or the owner has been told they are not.

## Step 7 — `aeri-library`: watching

ADR first (D6), with the licence exceptions. Then:

- A named thread owns a `notify` watcher over every root. Events are coalesced per directory for two seconds after the last one, then that directory is rescanned through Step 5. A hand-written trailing debounce, not a crate (D5).
- **Treat events as hints.** Whatever the event says, a rescan decides what changed. FSEvents on macOS may report a directory, not a file, and may merge or drop events; the rescan makes that harmless.
- If the watcher cannot be started (inotify's watch limit on Linux is the likely cause), log it once and carry on: the library works, it is just not live.

Done when: a test over a temporary root sees a new file arrive in the snapshot, and a removed one marked missing, each within a timeout and without a sleep used as synchronisation (wait on the snapshot channel). Check that the test passes on both CI platforms; if it is flaky on one, that is a failure, not a retry.

## Step 8 — `aeri-player`: owning the library, persisting state

ADR first (D8, D9): the change to the `Player` handle, and the persisted state.

```rust
#[derive(Debug)]
#[non_exhaustive]
pub struct PlayerConfig { pub database: PathBuf }

impl Player {
    pub fn open(config: PlayerConfig) -> Result<Self, PlayerError>;   // replaces with_default_output
    pub async fn scan(&self, root: PathBuf) -> Result<ScanReport, PlayerError>;
    pub fn library(&self) -> watch::Receiver<Arc<Snapshot>>;
}
```

- **The library's snapshot is passed through**, as `playback()` passes the engine's state (Phase 4, Step 7): the player thread never wakes for a scan batch.
- **Restore** at `open`: load the playlists, queue, and player state; resolve each entry's track through the snapshot; restore order, volume, and ReplayGain mode; put the current entry in place, paused at its position. A track that is missing stays in its playlist and is skipped when reached, as a failed file is today.
- **Persist** by D9. A write is a job to the library's writer; the player never waits for it on a command's path. On shutdown, the player sends the final position and waits for the writer to drain before the threads join.
- **`append`** now takes paths and reads tags through the library, so `play <paths…>` shows titles. Changing `append`'s argument is a second change to an existing method: include it in this ADR.
- **Test construction:** `with_fake_sink` gains a database path; tests use a temporary one.

Done when, against `FakeSink`: playlists, entries, the queue, the order, the volume, and the ReplayGain mode survive drop-and-reopen; ids allocated after reopening do not collide; a reopened player resumes the right entry at the saved position; an entry whose file has vanished is kept and skipped; and dropping every `Player` still joins every thread, now including the library's, under a timeout.

## Step 9 — M3U and M3U8 import and export

- **Import** reuses Phase 4's `parse_m3u`: a new playlist named after the file, with its entries' tracks read through the library.
- **Export** is a pure function from a playlist to M3U8 text (`#EXTM3U`, `#EXTINF` with duration and `ARTIST - TITLE`, one path per line), plus a command that writes it. Paths are relative to the playlist file when the track is beneath its directory, absolute otherwise. A path that is not UTF-8 cannot be written to an M3U8 file: skip it and report it.
- **Export never overwrites** an existing file unless told to (ADR 0030: a write is explicit). The CLI asks for `--force`.

Done when: the writer has unit and property tests, including that export followed by `parse_m3u` gives back the same sources, and the existing fuzz target is untouched.

## Step 10 — `aeri-cli`: `scan`, `play` with no paths, import, export

- `aeri scan <dir>` prints the `ScanReport` and exits.
- `aeri play` with no paths restores the last session (Step 8). With paths, as in Phase 4, appended to a playlist named for this run.
- `aeri import <file.m3u8>` and `aeri export <playlist name> <file.m3u8> [--force]`.
- `--database <path>` on every command; the default by D7.
- The progress line shows `ARTIST — TITLE` when the tags have them, the file name otherwise.

Done when: the D7 path resolution is a pure function of the environment with unit tests for both platforms' rules; integration tests drive `scan` then `play < /dev/null` over a temporary database and fixture tree; and export refuses to overwrite without `--force`.

## Step 11 — verify by use

Only the owner can do this. Record the result in the README's Status and the roadmap.

- Scan your own library. Compare the track count with what the filesystem holds, and look at a handful of albums with awkward tags: multiple artists, non-Latin text, compilations.
- Scan again: nothing changes, and it is fast.
- Add, retag (with another program), and delete a file while aeri is running; each is reflected without a manual scan.
- Quit mid-track with a queue and a shuffled order; `aeri play` resumes exactly there.
- Confirm no file under the root has a changed modification time after all of the above.

## What this brief verified

Checked with `cargo info` against crates.io on 2026-11-17. None of these crates is in `Cargo.lock` yet, so nothing below has been read in source.

- `rusqlite` 0.43.0, `MIT`, requires Rust 1.89 (the toolchain is 1.100.0). Its `bundled` feature enables `libsqlite3-sys/bundled`; its defaults are `cache` and `ffi-sqlite-wasm-rs`, both to be left off. `libsqlite3-sys` 0.40.0 is `MIT`.
- `lofty` 0.25.5, `MIT OR Apache-2.0`, requires Rust 1.89; its one default feature is `id3v2_compression_support` (pulls in a zlib decompressor), and it has an optional `serde` feature.
- `jwalk` 0.9.0, `MIT`; it depends on `rayon` 1.x and `crossbeam` 0.8 (`MIT OR Apache-2.0`).
- `rayon` 1.12.0, `MIT OR Apache-2.0`.
- `notify` 8.2.2 is the latest stable release (9.0.0 is at release candidate 9), `CC0-1.0`, requires Rust 1.88. `inotify` 0.11.5 and `inotify-sys` 0.1.8 are `ISC`; `fsevent-sys` 5.2.0 is `MIT`. That `notify` 8.2.2 depends on exactly these on each platform was **not** checked.
- `deny.toml`'s licence allowlist is `MIT`, `Apache-2.0`, `Unicode-3.0`, `Zlib`: neither `CC0-1.0` nor `ISC` is in it (D6).
- `Player::with_default_output` takes no arguments today (`crates/aeri-player/src/player.rs`), and `Track` is constructed in eleven files across `aeri-core`, `aeri-engine`, `aeri-player`, and `aeri-cli`.

Not verified: any crate's API, its publish date against the seven-day cooldown, its full dependency tree and their licences, or which of them have build scripts; that the bundled SQLite supports `STRICT` tables; how `lofty` maps keys and multiple values per format; that `lofty` reads without opening files for writing; how `notify` reports events on either platform; foobar2000's exact field names; and every signature above.

## When to stop and ask

Stop and raise it with the owner, do not improvise, when:

- a signature here cannot be made to compile without changing its *meaning*;
- you want a dependency this brief does not name, or one it names fails `cargo deny` for a reason D6 does not cover;
- the 100,000-track measurement misses a budget (D11);
- an existing `Player` method, `Engine` method, `OutputSink`, or `Decoder` needs to change beyond what Steps 3 and 8 specify;
- anything would write to a file aeri did not create, other than an export the user asked for;
- a check fails and the only fix you can see is to weaken the check.

## Not in this phase

Writing tags, renaming or moving files, and any other change to the user's files (ADR 0030, Curation). Recognising a moved file by its content. A library health report. Queries, search, filters, sorting, and autoplaylists, which need the script language (Phase 6). Cover art. Play counts and ratings. CUE sheets. Formats beyond FLAC and MP3. Settings storage beyond D9's player state. Controlling a running instance from another shell. If a step seems to need one of these, the step is being over-built.
