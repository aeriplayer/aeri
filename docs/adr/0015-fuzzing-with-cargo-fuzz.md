# ADR 0015 — Decoders are fuzzed with cargo-fuzz on a pinned nightly

- **Status:** Accepted
- **Date:** 2026-11-15

## Context

[`TESTING.md`](../TESTING.md) requires a fuzz target for anything that parses bytes aeri did not write, and Step 3 of the Phase 2–3 brief requires one for `aeri-decode` before the step is done. Media files in the wild are malformed; a decoder must not panic, hang, or allocate without bound on any input.

`cargo-fuzz` drives libFuzzer, which needs a nightly compiler for its sanitizer and coverage instrumentation. The alternatives were `afl.rs`, which needs its own toolchain wrapper and a separate instrumentation pass, and `cargo-bolero`, which fronts several engines but adds a layer the project does not need yet. `cargo-fuzz` is the common choice in the Rust ecosystem and the one `TESTING.md` already names.

## Decision

- **Layout.** Fuzz targets live in `fuzz/`, a Cargo workspace of its own, as `cargo-fuzz` expects. It is not a member of the root workspace, so stable builds, clippy and the feature powerset never compile libFuzzer. `fuzz/Cargo.lock` is committed.
- **Toolchain.** `fuzz/rust-toolchain.toml` pins a dated nightly (first `nightly-2026-11-01`), so a broken nightly cannot fail the job. It is bumped by hand, like the stable pin, to a nightly at least seven days old.
- **Entry point.** `aeri-decode` exposes `open_bytes` only under `cfg(fuzzing)`, which `cargo-fuzz` sets. It does not exist in normal builds, so it adds nothing to the public API. `cfg(fuzzing)` is declared in the workspace's `unexpected_cfgs` check-cfg.
- **The target.** `decode` feeds arbitrary bytes to `open_bytes` and calls `next_block` until the end or an error, seeded from `crates/aeri-decode/tests/fixtures/`. A crash or timeout found there becomes a committed regression fixture and a test.
- **Where it runs.** `cargo-fuzz` is pinned in `.config/cargo-tools.toml`. The run is the manual-stage `fuzz` hook, so local and CI runs share one definition. CI fuzzes for 60 seconds on every pull request that changes `fuzz/`, `aeri-decode`, `aeri-core`, or the root manifest or lockfile, and for 10 minutes on a daily schedule. Each input is limited to 10 seconds; libFuzzer's default memory limit (2 GiB RSS) stays in force.

## Consequences

- A second toolchain to install and keep current, on CI and on any machine that fuzzes.
- `fuzz/` is outside the root workspace, so `cargo deny`, `cargo-shear`, clippy and the cooldown check do not see it. Its one direct dependency, `libfuzzer-sys`, is licensed `(MIT OR Apache-2.0) AND NCSA` and compiles libFuzzer's C++ in a build script. That is acceptable only because nothing in `fuzz/` ships; revisit if a fuzz dependency ever reaches a release artefact. The seven-day cooldown for `fuzz/Cargo.lock` is checked by hand: on 2026-11-15 `cc` was held at 1.11.1 and `syn` at 3.0.9.
- The daily run starts from an empty corpus each time apart from the fixtures; nothing is persisted between runs. Persisting a corpus (as a cache or an artefact) is the next step if 10 minutes stops finding new coverage.
- A crashing input in CI is reported in the job log only; libFuzzer prints small inputs as Base64. Uploading artefacts is not set up.
- Revisit when a second parser (tags, playlists, scripts) needs fuzzing, which will add targets here rather than a new mechanism.
