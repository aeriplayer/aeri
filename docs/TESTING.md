# aeri — Testing and verification

How aeri earns confidence in its code. Every tool here runs from `prek.toml`, locally and in CI, so there is one definition of each check.

## The toolchain, for someone who knows Python's

| Concern | Python | aeri (Rust) | Notes |
| --- | --- | --- | --- |
| Lint | ruff | `clippy` (pedantic + nursery + a curated restriction set) | Levels in `Cargo.toml` `[workspace.lints]` |
| Format | ruff format | `rustfmt`, `taplo` (TOML), `rumdl` (Markdown) | |
| Type check | ty / mypy | the compiler | Types are not optional in Rust; the borrow checker also proves memory and data-race safety |
| Test runner | pytest | `cargo-nextest` | Each test in its own process; hung tests are killed; no retries |
| Doc examples | doctest | `cargo test --doc` | Every rustdoc example is compiled and run |
| Property tests | hypothesis | `proptest` | Shrinks failures to a minimal case and saves them as regressions |
| Mutation tests | mutmut | `cargo-mutants` | Proves the tests would notice a bug |
| Coverage | coverage.py | `cargo-llvm-cov` | Source-based, exact |
| Fuzzing | atheris | `cargo-fuzz` (libFuzzer) | For anything that parses untrusted bytes |
| Undefined behaviour | — | Miri | Interprets the program and detects UB in `unsafe` code |
| Concurrency model check | — | `loom` | Explores thread interleavings exhaustively |
| Dependency audit | pip-audit | `cargo-deny` | Advisories, licences, banned edges, sources, build-script allowlist |
| Dependency policy | — | `cargo xtask check-deps` | Explicit features; no stale build-script permissions |
| Unused dependencies | deptry | `cargo-shear` | Also finds misplaced and unused dev-dependencies |
| Feature matrix | tox | `cargo-hack` | Every combination of cargo features compiles |
| Spelling | codespell | `typos` | |

The last three rows of "verification" have no everyday Python counterpart, and they matter most here: aeri has `unsafe` FFI and a lock-free bridge to a real-time thread, which are exactly the things ordinary tests cannot see into.

## What runs when

| When | What | Command |
| --- | --- | --- |
| Every commit | format, lint, clippy, typos, unused deps, dependency policy, workflow lint | `prek run --all-files` |
| Every push | nextest, doctests, rustdoc, feature powerset, `cargo deny` | `prek run --all-files --stage pre-push` |
| Every pull request (CI) | all of the above on three targets, plus coverage, mutation testing of the changed lines, Miri, the crate cooldown, and `dist plan` when release inputs change; documentation-only changes skip the build jobs | see `.github/workflows/ci.yml` |
| Daily (CI) | `cargo deny` against new advisories | `.github/workflows/audit.yml` |
| On demand | coverage, mutants, Miri locally | `prek run <hook> --stage manual --all-files` |

Install the tools once with `cargo xtask install-tools`.

## Which technique for which code

| Code | Required | Why |
| --- | --- | --- |
| Pure logic with invariants (playlists, newtypes, position maths, the script evaluator) | Unit tests **and** property tests | Hand-picked examples miss the sequence of operations that breaks an invariant; `proptest` finds it |
| Anything parsing bytes we did not write (decoders, tags, playlist files, scripts) | Fixture tests **and** a fuzz target | Media files in the wild are malformed; a decoder must never panic or loop forever |
| `unsafe` code and its safe wrappers | Unit tests run under Miri | Miri catches use-after-free, bad aliasing, and uninitialised reads that pass normal tests |
| Lock-free and atomic code (the ring-buffer bridge, flush generation) | A `loom` model test | Races depend on interleavings that never occur on a developer machine |
| The real-time callback body | Unit tests under an allocation guard | An allocation on the audio thread is a glitch, not a test failure, unless a test makes it one |
| The engine | Integration tests against `FakeSink` | Deterministic; no device, no timing |
| Device sinks | Manual listening, plus the above for their callback | CI runners have no audio hardware |
| Hot paths (decode throughput, DSP nodes, library scan) | Benchmarks, from Phase 4 | A regression needs a number to be noticed |

## Standards

- **Mutation testing is the measure of test quality, not coverage.** Coverage says a line ran; a surviving mutant says no test would notice if that line were wrong. Every mutant in changed code must be caught, or explicitly justified in `.cargo/mutants.toml` with a reason.
- **Coverage is a floor, not a target.** The gate is 85% of lines across the workspace, and 100% for crates that are pure logic with no I/O (`aeri-core`, later `aeri-script`), where every line is reachable from a test. Do not write tests to move the number; write them to pin behaviour, and let the number follow.
- **No flaky tests.** No retries, no sleeps as synchronisation, no dependence on test order or wall-clock time. A test that fails one run in a hundred is failing.
- **A bug fix starts with a failing test** that reproduces it.
- **Property tests commit their regressions.** `proptest` writes failing cases to `proptest-regressions/`; those files are checked in.
- **Tests that cannot run under Miri** (FFI, real I/O) are marked `#[cfg_attr(miri, ignore)]` with a comment saying why. Everything else runs.
- **Fixtures are small, committed, and reproducible:** the generating command sits in a `README.md` beside them.

## Not yet in place

Recorded here so the gap is visible rather than assumed away.

- **Fuzzing and `loom`** have no code to act on yet. They are required by the design brief at the step that introduces a decoder and the real-time bridge. Fuzz targets are then built on every pull request, so they cannot stop compiling, and run on a daily schedule. `cfg(fuzzing)` and `cfg(loom)` are declared in `unexpected_cfgs`'s `check-cfg` in the same change.
- **Benchmarks** arrive with the DSP chain in Phase 4, built with the `profiling` profile. CI runs them under CodSpeed's instruction-counting mode, which gives stable numbers on shared runners where wall-clock timings are noise.
- **Device sinks on Linux** cannot be exercised in CI and need a real machine.
- **Coverage on macOS-only code** is not measured: the coverage job runs on Linux.
