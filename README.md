# aeri

A music player written in Rust for macOS and Linux, in the tradition of foobar2000: playback built around playlists, a music library, one small script language for formatting and searching, and an interface assembled from widgets in a layout you control.

## Status

On macOS, `aeri play <file>` plays a FLAC or MP3 file through the default output. Phase 2 (core playback types) is complete. **Phase 3, first audio, is complete apart from the Linux sink**, specified step by step in
[`docs/design/phase-2-3-playback.md`](docs/design/phase-2-3-playback.md):

- Steps 1–6 are done: the core types; `OutputSink` and `FakeSink`; the FLAC and MP3 decoder, fuzzed; `CoreAudioSink`; the playback engine; and `aeri play`, each checked by hand where the brief asks.
- **Remaining: the PipeWire sink for Linux**, its own step, which needs a Linux machine. Until then `aeri play` fails on Linux with a readable error.

Five crates exist: `aeri-core`, `aeri-audio`, `aeri-decode`, `aeri-engine`, `aeri-cli`. `aeri-core` holds the playback types, `aeri-audio` the `OutputSink` trait, `CoreAudioSink`, and the test `FakeSink`, `aeri-decode` FLAC and MP3 decoding, `aeri-engine` the playback actor, and `aeri-cli` the `aeri` binary. Planned crates are created in the phase that first needs them, not scaffolded early.

Update this section whenever a step or phase completes.

## Where things are written down

| Document | What it settles |
| --- | --- |
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | Pillars, crate graph, threading model, locked decisions, open decisions |
| [`docs/ROADMAP.md`](docs/ROADMAP.md) | Phases and their checklists |
| [`docs/design/`](docs/design/) | The step-by-step brief for the current phase |
| [`docs/adr/`](docs/adr/README.md) | Every decision and why; when a new ADR is required |
| [`docs/RUST_STYLE.md`](docs/RUST_STYLE.md) | How Rust is written here, with reasons |
| [`docs/TESTING.md`](docs/TESTING.md) | Which verification each kind of code requires, and the tools |
| [`docs/TOOLING.md`](docs/TOOLING.md) | Why the hooks, CI, lints, and dependency policy are configured as they are |

Read the architecture, the current brief, and the ADRs it cites before writing code.

## Getting started

```bash
# Once per clone
brew install prek
cargo xtask install-hooks
cargo xtask install-tools   # every tool in .config/cargo-tools.toml

# Commit-stage checks over the whole tree (hygiene, fmt, lints, clippy)
prek run --all-files

# Push-stage checks (tests, docs, dependency policy)
prek run --all-files --stage pre-push

# One hook
prek run cargo-clippy --all-files

# On-demand checks that CI runs on every pull request
prek run coverage --stage manual --all-files
prek run mutants --stage manual --all-files
prek run miri --stage manual --all-files   # needs a nightly toolchain with miri
prek run loom --stage manual --all-files
FUZZ_SECONDS=600 prek run fuzz --stage manual --all-files   # needs the nightly in fuzz/rust-toolchain.toml

# Move pinned hooks to their latest releases by hand (Renovate normally does this)
prek update
```

Formatters rewrite files and fail the commit; re-stage and commit again.

## How work is done here

This applies to people and to coding agents alike.

1. **Start** with the Status section above, the current design brief, and the ADRs it cites. Run `git status` and `git log -5`.
2. **Work on a branch**, never `main`: one branch per phase (`phase-3`), merged before the next phase begins. One brief step per commit.
3. **Follow the brief.** Build the types and traits it specifies. Do not add features, abstractions, or crates it does not ask for.
4. **Verify, do not recall.** Before calling a third-party API or naming a version, read the docs for the version in `Cargo.lock` or the one `cargo add` resolves. Memory of crate APIs is frequently out of date.
5. **Add a dependency only with the code that uses it.** Unused ones fail the build.
6. **Finish** only when `prek run --all-files` and `prek run --all-files --stage pre-push` both pass, new behaviour has tests, public items have rustdoc, and Status and the roadmap checklist are updated.
7. **Keep comments rare.** Write one only if deleting it would let a reader make a mistake they cannot see coming, in at most two lines (RUST_STYLE §12). Longer explanations go in rustdoc, `docs/`, or an ADR.
8. **Report honestly.** Say what was not verified, what was not run, and what you were unsure of. An unverified claim stated as fact is worse than a gap.

### Check with the owner before

- weakening, skipping, or suppressing any check: a lint level, a hook, a `deny.toml` rule, `--no-verify`, or a crate-wide `#[expect]`. A failing check means the code changes.
- contradicting an accepted ADR, a locked decision, or a pillar.
- adding a dependency the brief does not name, or a new crate.
- copying or translating code from another project.
- writing `unsafe` outside `aeri-audio`, or allocating, locking, logging, or blocking in the real-time audio callback.
- changing a public trait that crosses a crate boundary (`OutputSink`, `Decoder`, `DspNode`), or an existing method of the `Player` handle.
- pushing, merging to `main`, rewriting history, or tagging a release.

When a decision of that kind is agreed, record it as an ADR before writing the code (see [`docs/adr/README.md`](docs/adr/README.md)).

## Rules the tooling enforces

These fail the commit or the push; you do not need to remember them, only not to fight them.

- No `unwrap()`, `todo!()`, `dbg!()`, or `println!` outside tests; no `unsafe` without opting in; every `unsafe` block has a `// SAFETY:` comment; public items are documented and `Debug`; suppressions are `#[expect(lint, reason = "…")]`, never `#[allow]`.
- Files go through `fs_err`, threads through `thread::Builder` with a name (`clippy.toml`).
- Every external dependency sets `default-features = false` and lists its features; crates with build scripts are named in `deny.toml`; versions younger than seven days are refused.
- Only the engine depends on `aeri-audio` and `aeri-decode`. Frontends reach the application through `aeri-player` once it exists; until then `aeri-cli` alone may depend on `aeri-engine` (`deny.toml`, ADR 0006).
- No unused dependencies; licences limited to the `deny.toml` allowlist. Change the lockfile with `cargo update --precise`, never a bare `cargo update`.
- Conventional Commits, scoped by crate, imperative subject of at most 72 characters, type one of `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `build`, `ci`, `chore`, `revert`. Commits are signed. No AI attribution anywhere in a commit.
- No commits on `main`.

```text
feat(engine): preload next track 5s before end of current
fix(decode): skip corrupt packets instead of ending the track
docs(adr): record ring buffer sizing rationale
```

## Toolchain

- Edition 2024. `rust-toolchain.toml` pins an exact stable so CI cannot break on an unrelated release. Bump it in its own commit with every check green.
- `rustfmt.toml` is defaults at edition 2024. Import grouping is a review convention: the rustfmt options for it are nightly-only.
- Hook tools written in Rust build with prek's own newer toolchain, not the pinned one.

## Testing

- Unit tests in `#[cfg(test)] mod tests` beside the code; cross-module behaviour in each crate's `tests/`. Files in `tests/` wrap their tests in `#[cfg(test)] mod tests` too; a lint requires it.
- `aeri-engine` is tested against `FakeSink`, never a real device. Device sinks are verified by hand and by unit tests of the callback body.
- Fixtures are small committed files with the command that generated them beside them.

## Distribution

`cargo-dist` drives releases from `dist-workspace.toml`: signed and notarized `.dmg` on macOS, an AUR `PKGBUILD` in `pkg/aur/`, a Homebrew formula in `pkg/homebrew/`. Nothing has been released yet. The release workflow has never run end to end; CI runs `dist plan` against its configuration on every pull request that touches it.

## Licence

`MIT OR Apache-2.0`, declared by every crate. See [`LICENSE-MIT`](LICENSE-MIT) and
[`LICENSE-APACHE`](LICENSE-APACHE).
