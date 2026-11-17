<!-- rumdl-disable MD033 MD041 -->
<p align="center">
  <img src="docs/assets/logo-v1.svg" width="160" alt="The aeri logo: a round maple tabletop with a blue resin river running through it">
</p>

<h1 align="center">aeri</h1>

<p align="center">A music player for macOS and Linux in the tradition of <a href="https://www.foobar2000.org/">foobar2000</a>, written in Rust.</p>
<!-- rumdl-enable MD033 MD041 -->

> [!NOTE]
> aeri is in early development. Today it plays FLAC and MP3 files, directories of them, and M3U playlists from the command line. Nothing has been released.

## What aeri is for

aeri is for people who keep a music library and want a player built around it:

- playback organised around playlists;
- a music library you can search;
- one small script language for how tracks are shown and how they are searched;
- an interface assembled from widgets, in a layout you control.

None of those exist yet. The [roadmap](docs/ROADMAP.md) says when each arrives.

aeri supports Apple Silicon Macs, and Linux with PipeWire. No support for Windows is planned.

## What works today

```sh
aeri play song.flac
aeri play --order shuffle ~/Music/album-a ~/Music/mix.m3u song.flac
```

plays every FLAC and MP3 file it is given, one after another and gaplessly where the files share a sample rate, through the default output, and shows the file name and position until the last one ends or you press Ctrl-C. A path can be a file, a directory (searched, subdirectories included, in path order), or an `.m3u` or `.m3u8` playlist. A file that cannot be played is reported on standard error and skipped.

| Flag | Values |
| --- | --- |
| `--order` | `default`, `repeat-track`, `repeat-playlist`, `shuffle` |
| `--volume` | decibels below full volume, such as `-6` |
| `--replaygain` | `off`, `track`, `album` (the default) |

- **On macOS** aeri switches the output device to the file's sample rate when the device offers it, so CoreAudio does not resample. It never takes the device for itself, so other apps keep playing.
- **On Linux** aeri asks PipeWire for the file's sample rate. PipeWire resamples when the system's audio graph runs at another rate, which is common.

## Status

Phase 3, first audio, is complete: decoding, the playback engine, and native output on both platforms. Phase 4 is in progress: playlists, a queue, playback order, volume and ReplayGain work through the player, and the command line takes paths, directories and M3U playlists. Interactive keys, and listening checks on both platforms, remain. See the [roadmap](docs/ROADMAP.md).

## Building from source

You need [rustup](https://rustup.rs/). The toolchain pinned in `rust-toolchain.toml` installs itself on first build.

On Linux you also need PipeWire's headers and libclang, because aeri generates its PipeWire bindings at build time:

```sh
sudo apt install libpipewire-0.3-dev libclang-dev   # Debian, Ubuntu
sudo pacman -S libpipewire clang                     # Arch
```

Then:

```sh
git clone https://github.com/aeriplayer/aeri
cd aeri
cargo run -p aeri-cli -- play path/to/song.flac
```

## Contributing

| Document | What it covers |
| --- | --- |
| [`AGENTS.md`](AGENTS.md) | Rules for working on aeri, apply to people and agents alike |
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | Pillars, crate graph, threading model, and the decisions still open |
| [`docs/ROADMAP.md`](docs/ROADMAP.md) | Phases and their checklists |
| [`docs/adr/`](docs/adr/README.md) | Every architectural decision and why it was made |
| [`docs/design/`](docs/design/) | The step-by-step brief for the current phase |
| [`docs/RUST_STYLE.md`](docs/RUST_STYLE.md) | How Rust is written here |
| [`docs/TESTING.md`](docs/TESTING.md) | What verification each kind of code needs |
| [`docs/TOOLING.md`](docs/TOOLING.md) | Why the hooks, CI, lints, and dependency policy are set up as they are |

## Licence

aeri is licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
