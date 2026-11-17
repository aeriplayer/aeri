# ADR 0021 — The allocation guard lives in a shared, test-only crate

- **Status:** Accepted
- **Date:** 2026-11-16

## Context

[ADR 0018](0018-real-time-verification.md) put the allocation guard inside `aeri-audio`'s unit tests: a counting `#[global_allocator]`, about sixty lines, whose `unsafe impl GlobalAlloc` is the only `unsafe` in the crate that exists only for tests.

`aeri-dsp` ([ADR 0020](0020-aeri-dsp-in-place-interleaved-chain.md)) needs the same guard for its `process` bodies, and `TESTING.md` requires it for real-time code. `aeri-dsp` may not depend on `aeri-audio`, and its `unsafe` rule says `unsafe` lives only there.

Alternatives considered:

- **Copy the guard into `aeri-dsp`.** Two copies of an `unsafe impl` to keep correct, and `unsafe` outside `aeri-audio` anyway.
- **Export the guard from `aeri-audio` and test the nodes from `aeri-engine`.** Keeps `unsafe` where it is, but moves the nodes' tests away from the nodes, and `cargo mutants -p aeri-dsp` would no longer see them.
- **A third-party allocation-counting crate.** Not evaluated; ADR 0018 already found the obvious candidate unmaintained and outside the licence allowlist.

## Decision

- A new crate, `aeri-alloc-guard`, with `publish = false`, holds the guard moved from `aeri-audio`: `assert_no_alloc`, the counting allocator, and the guard's own tests.
- It is a **dev-dependency only**, of `aeri-audio` and `aeri-dsp`. `deny.toml` names those two as its wrappers.
- It is the one place outside `aeri-audio` that may contain `unsafe`, and only the `GlobalAlloc` impl. Each call keeps its own `// SAFETY:` comment.
- ADR 0018's decision about the guard's location is superseded by this one. Its `loom` decision stands.

## Consequences

- Any test binary that links the crate gets the counting allocator for the whole process. Guarded sections are per thread, so unrelated tests are unaffected, but each test run pays a thread-local read on every allocation.
- Nothing mechanical stops a future crate from depending on it outside `[dev-dependencies]`; the `deny.toml` wrapper list and review are the only guards. A normal dependency would install a counting allocator in a release binary.
- A second crate is created, and the architecture's rule that `unsafe` lives only in `aeri-audio` now has this test-only exception.
- Not verified: that `cargo-shear` and `cargo xtask check-deps` accept a dev-only crate with no non-test code; check when it is created.
- Revisit if a maintained, permissively licensed crate offers the same guard, which would remove the crate and its `unsafe`.
