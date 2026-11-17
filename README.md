<!-- rumdl-disable MD033 MD041 -->
<p align="center">
  <img src="docs/assets/logo-v1.svg" width="160" alt="The aeri logo: a round maple tabletop with a blue resin river running through it">
</p>

<h1 align="center">aeri</h1>

<p align="center">A music library, looked after properly. Plays like <a href="https://www.foobar2000.org/">foobar2000</a>, written in Rust.</p>
<!-- rumdl-enable MD033 MD041 -->

> [!NOTE]
> aeri is in early development. Today it plays FLAC and MP3 files, directories of them, and M3U playlists from the command line. Nothing has been released.

## What aeri is

Most players treat your library as a pile of files to play. aeri treats it as something you curate: scanned, tagged, searched, sorted and kept tidy over years, with playback as one thing you do with it.

foobar2000 is the starting point, because it got the foundations right: playlists first, tags you can trust, and everything about how music is shown or found is yours to define. aeri begins there, then aims further.

## How aeri is meant to work

Good defaults, and every one of them overridable. [ruff](https://docs.astral.sh/ruff/) is the model: it works the moment you install it, and nothing it does is beyond your say.

- **Works on first run.** Point aeri at a directory and the library, the layout and the columns make sense without a settings screen.
- **Nothing is hard-coded that could be yours.** Columns, groupings, filters and searches are small scripts. The interface is a layout you can rearrange. A default is only a starting script or layout that you can edit or replace.
- **Nothing changes unless you say so.** aeri never rewrites your tags, renames your files or reorganises your folders on its own. Any change to your files is a request you make, previewed first.

None of this exists yet beyond the player. The [roadmap](docs/ROADMAP.md) says when each part arrives.

aeri supports Apple Silicon Macs, and Linux with PipeWire. No support for Windows is planned.

## What works today

```sh
aeri play song.flac
aeri play --order shuffle ~/Music/album-a ~/Music/mix.m3u song.flac
```

plays every FLAC and MP3 file it is given, one after another and gaplessly where the files share a sample rate, through the default output, and shows the file name, position, order and volume until the last one ends or you quit. A path can be a file, a directory (searched, subdirectories included, in path order), or an `.m3u` or `.m3u8` playlist. A file that cannot be played is reported on standard error and skipped.

In a terminal these keys work while it plays:

| Key | Does |
| --- | --- |
| Space | pause or resume |
| `n`, `p` | next, previous |
| Left, Right | seek back or forward 5 seconds |
| Down, Up | volume down or up 2 dB |
| `o` | cycle the order |
| `s` | stop after the current track, or cancel that |
| `q`, Ctrl-C | quit |

With input piped in, there are no keys and `aeri` plays through and exits.

| Flag | Values |
| --- | --- |
| `--order` | `default`, `repeat-track`, `repeat-playlist`, `shuffle` |
| `--volume` | decibels below full volume, such as `-6` |
| `--replaygain` | `off`, `track`, `album` (the default) |

- **On macOS** aeri switches the output device to the file's sample rate when the device offers it, so CoreAudio does not resample. It never takes the device for itself, so other apps keep playing.
- **On Linux** aeri asks PipeWire for the file's sample rate. PipeWire resamples when the system's audio graph runs at another rate, which is common.

## Status

Phase 3, first audio, is complete: decoding, the playback engine, and native output on both platforms. Phase 4 is in progress: playlists, a queue, playback order, volume and ReplayGain work through the player, and the command line takes paths, directories and M3U playlists. The owner's listening checks (gapless FLAC and MP3, sample-rate changes, volume, ReplayGain, skipping and seeking, an unplayable file) passed on macOS on 2026-11-17. The Linux listening check is still owed. See the [roadmap](docs/ROADMAP.md).

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
