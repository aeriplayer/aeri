# ADR 0018 — The callback is verified by an in-house allocation guard and a loom model

- **Status:** Accepted; the allocation guard's location is superseded by [ADR 0021](0021-shared-allocation-guard-crate.md)
- **Date:** 2026-11-16

## Context

[`TESTING.md`](../TESTING.md) requires the real-time callback body to be tested under an allocation guard, and lock-free code to have a `loom` model test. The Step 4 brief suggests the `assert_no_alloc` crate for the guard. When checked on 2026-11-16 its last release was 1.1.2 in August 2021, and its licence, BSD-1-Clause, is not on `deny.toml`'s allowlist.

`loom` explores thread interleavings, but only of synchronisation built from `loom`'s own types. `rtrb` uses `std` atomics, so `loom` cannot look inside it.

## Decision

- **Allocation guard.** `aeri-audio`'s unit tests install a counting `#[global_allocator]` that forwards to the system allocator. `assert_no_alloc(|| …)` marks the current thread, runs the closure, and fails the test if it allocated, reallocated, or freed. The mark is a `const`-initialised thread-local `Cell`, which needs no allocation of its own. This is the only `unsafe` in `aeri-audio` so far (`unsafe impl GlobalAlloc`), and it exists only in test builds.
- **loom.** `loom` 0.7 is a dev-dependency behind `cfg(loom)`, declared in the workspace's `check-cfg`. Under `cfg(loom)` the bridge's atomics and `Arc` come from `loom::sync`, and the ring is a small `loom`-visible stand-in with `rtrb`'s semantics (bounded, FIFO, release on push, acquire on pop). The model checks the flush and pause protocol of [ADR 0016](0016-real-time-bridge.md): no device buffer mixes pre-flush and post-flush audio, every post-flush sample plays exactly once and in order, and pause loses and repeats nothing. (Old audio following new is ruled out by the ring being FIFO, so it is not the property under test.) Swapping the callback's two reads makes the first property fail under `loom`, which shows the model can see the ordering it depends on.
- **Where it runs.** A manual-stage `loom` prek hook runs the model in release mode. CI runs it on every pull request that changes `aeri-audio`.

## Consequences

- No dependency for the guard, and no licence exception. It is about forty lines that must stay correct; its own test checks that it catches an allocation.
- The `loom` model verifies aeri's protocol, not `rtrb`. `rtrb`'s correctness rests on its own tests and Miri runs; aeri's Miri run exercises the bridge over the real `rtrb`.
- The bridge is generic over a private ring trait so the stand-in can replace `rtrb` under `loom`. That trait is the cost of model-checking; it is not public.
- The model is small (a few samples, two threads) to keep `loom`'s state space tractable. A protocol bug that needs a longer history could escape it.
- Revisit if `rtrb` gains `loom` support, which would let the model run against the real ring.
