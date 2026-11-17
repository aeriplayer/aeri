# AGENTS.md

How work is done on aeri: for coding agents, and for people, who follow the same rules. Users of aeri start at [`README.md`](README.md).

aeri is a music player for macOS and Linux in the tradition of foobar2000, written in Rust and built mostly by agents working with one maintainer, the owner. It is early: read the Status section of the README before anything else.

## House rules

1. **Verify, don't recall.** Before calling a third-party API or naming a version, read the docs or source for the version in `Cargo.lock`, or the one `cargo add` resolves. Memory of crate APIs is often out of date. Where behaviour matters, probe it, and record what you found.
2. **Follow the brief.** Build the types and traits the current design brief specifies. Do not add features, abstractions, or crates it does not ask for. When code and brief disagree after you have tried it, stop and raise it.
3. **A failing check means the code changes.** Never weaken, skip, or suppress a check to get past it: no lint level change, `--no-verify`, `deny.toml` relaxation, crate-wide `#[expect]`, or mutant exclusion without the owner's agreement.
4. **Report only what ran.** Say what you verified, what you did not run, and what you are unsure of. An unverified claim stated as fact is worse than a gap.
5. **Leave the tree green.** One brief step per commit. Sessions end without warning; a passing tree and an honest Status section are how the next session picks up.
6. **Keep comments rare.** Write one only if deleting it would let a reader make a mistake they cannot see coming, in at most two lines ([RUST_STYLE §12](docs/RUST_STYLE.md#12-comments)). Longer explanations go in rustdoc, `docs/`, or an ADR.
7. **No agent attribution.** No `Co-authored-by`, "Generated with", or similar in commits, pull requests, or code. The `commit-msg` hook rejects the common forms.
8. **Ask the owner first** before any of the following. Once a decision is agreed, record it as an ADR before writing the code ([`docs/adr/README.md`](docs/adr/README.md)).
   - weakening a check (rule 3);
   - contradicting an accepted ADR, a locked decision, or a pillar;
   - adding a dependency the brief does not name, or a new crate;
   - copying or translating code from another project;
   - writing `unsafe` outside `aeri-audio`, or allocating, locking, logging, or blocking in the real-time audio callback;
   - changing a public trait that crosses a crate boundary (`OutputSink`, `Decoder`, `DspNode`), or an existing method of the `Player` handle;
   - pushing, merging to `main`, rewriting history, tagging a release, or changing anything on GitHub.

## 1. Orientation

Run `git status` and `git log -5`, then read:

| Read | Why |
| --- | --- |
| [`README.md`](README.md) › Status | Where the project is |
| [`docs/design/`](docs/design/) | The brief for the current phase, step by step |
| The ADRs that brief cites | The decisions it rests on, and their costs |
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | Pillars, crate graph and dependency rules, threading model, open decisions |
| [`docs/RUST_STYLE.md`](docs/RUST_STYLE.md) | How Rust is written here, with reasons |
| [`docs/TESTING.md`](docs/TESTING.md) | Which verification each kind of code requires |
| [`docs/TOOLING.md`](docs/TOOLING.md) | Why the hooks, CI, lints, and dependency policy are configured as they are |

## 2. Workspace map

```text
crates/
  aeri-core     playback types; depends on no other workspace crate
  aeri-audio    the OutputSink trait, CoreAudioSink, PipeWireSink, the real-time bridge,
                and FakeSink behind the test-util feature; the only crate with unsafe
  aeri-decode   FLAC and MP3 decoding over symphonia
  aeri-dsp      the DspNode trait, the chain, and the node registry
  aeri-engine   the playback actor: decode, then sink; a current track and a next slot
  aeri-cli      the `aeri` binary
xtask/          cargo xtask: install-hooks, install-tools, check-deps
fuzz/           cargo-fuzz targets for the decoder, in their own workspace (ADR 0015)
docs/assets/    the logo, and the scripts that generate it
```

The crate graph is enforced by `deny.toml`: only `aeri-engine` depends on `aeri-audio`, `aeri-decode`, and `aeri-dsp`, and until `aeri-player` exists only `aeri-cli` depends on `aeri-engine` (ADR 0006). Planned crates are created in the phase that first needs them, never scaffolded early; the full graph is in [`ARCHITECTURE.md`](docs/ARCHITECTURE.md#crate-graph).

## 3. Golden rules

### The real-time audio callback

The callback is the most constrained code in aeri, and the tooling cannot fully guard it. Inside it there is no allocation or deallocation, no lock, channel, or `OnceLock` initialisation, no I/O or other syscall beyond the audio API's own, no logging, and nothing that can panic: no out-of-bounds indexing, no `unwrap`, no arithmetic that can overflow. Its inputs are the ring's consumer and the atomics of [ADR 0016](docs/adr/0016-real-time-bridge.md).

Test the callback body as a plain function under the allocation guard of [ADR 0018](docs/adr/0018-real-time-verification.md), and run it under Miri. A guard only catches the paths a test reaches.

### Rules the tooling enforces

These fail the commit or the push. You do not need to remember them, only not to fight them.

- No `unwrap()`, `todo!()`, `dbg!()`, or `println!` outside tests. No `unsafe` without opting in on the item; every `unsafe` block has a `// SAFETY:` comment. Public items are documented and `Debug`. Suppressions are `#[expect(lint, reason = "…")]`, never `#[allow]`.
- Files go through `fs_err`, and threads through `thread::Builder` with a name (`clippy.toml`).
- Every external dependency sets `default-features = false` and lists its features. Crates with build scripts are named in `deny.toml`. Versions younger than seven days are refused.
- No unused dependencies. Licences are limited to the `deny.toml` allowlist and its per-crate exceptions. Change the lockfile with `cargo update --precise`, never a bare `cargo update`.
- Commits follow Conventional Commits (below), are signed, and never land on `main`.

## 4. Commands

```sh
# Once per clone
cargo xtask install-hooks
cargo xtask install-tools                  # every tool in .config/cargo-tools.toml

prek run --all-files                       # commit stage: hygiene, fmt, lints, clippy
prek run --all-files --stage pre-push      # push stage: tests, docs, dependency policy
prek run cargo-clippy --all-files          # one hook

# On demand; CI runs these on every pull request
prek run coverage --stage manual --all-files
prek run mutants --stage manual --all-files
prek run miri --stage manual --all-files   # needs a nightly with miri
prek run loom --stage manual --all-files
FUZZ_SECONDS=600 prek run fuzz --stage manual --all-files
```

Formatters rewrite files and fail the commit; re-stage and commit again. The `mutants` hook diffs committed history against `origin/main`; to test uncommitted work, run `git diff HEAD > target/mutants.diff && cargo mutants --in-diff target/mutants.diff`.

Building on Linux needs PipeWire's headers and libclang (README › Building from source).

## 5. Before you finish

- `prek run --all-files` and `prek run --all-files --stage pre-push` both pass.
- New behaviour has tests, at the level [`TESTING.md`](docs/TESTING.md) requires for that kind of code. If you touched `aeri-audio`, the `loom` and `miri` hooks pass too.
- Public items have rustdoc.
- The README's Status section and the roadmap checklist say what is now true.
- Device sinks are checked by listening, which only the owner can do. Say that it is still owed.
- Your report says what you did not run or could not verify.

## 6. Commits and branches

Work on a branch, never `main`: one branch per phase (`phase-3`), merged before the next phase begins, and one brief step per commit.

Subjects follow Conventional Commits, scoped by crate, imperative, at most 72 characters. The type is one of `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `build`, `ci`, `chore`, `revert`.

```text
feat(engine): preload next track 5s before end of current
fix(decode): skip corrupt packets instead of ending the track
docs(adr): record ring buffer sizing rationale
```

## 7. Where things are tracked

- **README › Status:** what works now. Update it whenever a step or phase completes.
- **[`docs/ROADMAP.md`](docs/ROADMAP.md):** phases and their checklists.
- **[`docs/design/`](docs/design/):** the current phase's brief.
- **[`docs/adr/`](docs/adr/README.md):** every decision, why, and what it costs. Accepted ADRs are never rewritten; a new one supersedes them.
- **[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) › Open decisions:** what must be settled before the crate that needs it.

## 8. Keeping this file useful

Every agent session reads this file, so it stays short and high-signal.

- Editing or clarifying a rule is always welcome. A **new** rule must be non-obvious (someone who knows the code would still get it wrong), have come up more than once, and be specific enough to act on.
- Rules are traps to avoid, not maps to follow. Module layouts and data flow go stale; they belong in `ARCHITECTURE.md` and the ADRs, or are read from the code.
- No drive-by additions. Propose a rule in your report; the owner decides, and it lands in its own commit with the reason it exists.
