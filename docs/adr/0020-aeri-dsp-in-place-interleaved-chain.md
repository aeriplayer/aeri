# ADR 0020 — `aeri-dsp` holds an interleaved, in-place chain of compiled-in nodes

- **Status:** Accepted
- **Date:** 2026-11-16

## Context

Phase 4 adds volume and ReplayGain between the decoder and the sink. [ADR 0002](0002-compiled-in-extension-points.md) makes `DspNode` a trait with a registry, and `ARCHITECTURE.md` left two things open: whether the chain's buffers are interleaved or planar, and what a node may change about the stream.

The decoder and the sink are both interleaved. The only nodes of this phase are per-sample gains, which do not care about layout. A later equaliser would prefer planar for its filters, but it can convert inside itself.

Gapless playback joins two tracks iff their decoders' `StreamSpec`s are equal ([ADR 0005](0005-engine-plays-current-and-next.md)). If a node could change the rate or channel count, that rule would depend on the chain as well as the files.

## Decision

- **A new crate, `aeri-dsp`,** holds `DspNode`, `Chain`, and `Registry`. It depends on `aeri-core` only. Only `aeri-engine` may depend on it; `deny.toml` gets `{ crate = "aeri-dsp", wrappers = ["aeri-engine"] }`.
- **Buffers are interleaved `f32`.** No conversion happens between the decoder, the chain, and the sink.
- **A node processes in place and never changes the stream spec.** Frames in equal frames out. Anything that changes rate or channel count is a pipeline stage beside the chain, not a node.
- **The chain splits each block into pieces of at most 1024 frames** before calling its nodes. The engine hands over whatever the decoder produced; the locked block size of `ARCHITECTURE.md` holds because of this split.
- **Settings are a closed `Control` enum,** sent to every node, each ignoring what is not its own. Every node is compiled in, so the compiler can check each use.
- **The registry maps a `&'static str` id to a constructor** `fn() -> Box<dyn DspNode>`, and builds a `Chain` from a list of ids, failing on an unknown one.
- **`process` allocates nothing.** Its tests run under the allocation guard of [ADR 0021](0021-shared-allocation-guard-crate.md).

## Consequences

- A planar node, such as an FFT equaliser, pays its own conversions. Revisit if two such nodes appear, since each would convert separately.
- A node that must change the spec, such as a resampler, cannot be written as a `DspNode`. It needs a stage beside the chain; that is deliberate, and costs a second extension point when the first caller exists.
- Adding a setting means adding a `Control` variant, which every node's `match` must then consider. A third-party node cannot add one; nodes are compiled in, so this is accepted. Saving a chain's settings waits for the settings-storage decision.
- `Box<dyn DspNode>` costs one indirect call per node per block, which is negligible beside the per-sample work.
- Ids are not yet a compatibility surface, because nothing persists a chain in this phase. They become one when settings storage lands.
