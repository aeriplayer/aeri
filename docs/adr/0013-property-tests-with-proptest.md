# ADR 0013 — Property tests use proptest

- **Status:** Accepted
- **Date:** 2026-11-15

## Context

[`TESTING.md`](../TESTING.md) requires property tests for pure logic with invariants: newtypes now, and later playlists, position maths and the script evaluator. Step 1 of the Phase 2–3 brief adds the first such code (`SampleRate`, `Channels`), and the brief does not name a property-testing crate.

The alternatives on the table were `proptest`, `quickcheck`, and hand-written sweeps over the input space. `quickcheck` shrinks by type rather than by strategy, so a failure in a composite input shrinks poorly, and it has no built-in regression files. Hand-written sweeps cover a `u16` exhaustively but cannot cover a `u32` or a sequence of playlist operations.

## Decision

- Property tests are written with `proptest`, declared once in `[workspace.dependencies]` with `default-features = false` and only `std`. A crate adds it as a dev-dependency with its first property test; `aeri-core` is the first.
- The default `fork` and `timeout` features (which pull in `rusty-fork` and `tempfile`) stay off: nextest already runs each test in its own process and kills hung ones.
- Failing cases are written to `proptest-regressions/` and committed, as `TESTING.md` requires.

## Consequences

- Three crates with build scripts enter the dev-dependency graph and are named in `deny.toml`: `getrandom` (detects the memory sanitizer), `libc` (reads `rustc --version` and target cfgs; runs `emcc` only for Emscripten targets, and `freebsd-version` only under its own `LIBC_CI`), and `num-traits` (one `autocfg` probe). Each build script was read in full on 2026-11-15, for the versions in `Cargo.lock`.
- `proptest` 1.12.0 resolves `rand` 0.10.4, released on the day of this decision; the lockfile holds `rand` at 0.10.3 until 0.10.4 clears the seven-day cooldown.
- Dev-dependencies only: nothing here ships in a binary.
- Revisit if `proptest` stops being maintained, or if its build-script footprint grows.
