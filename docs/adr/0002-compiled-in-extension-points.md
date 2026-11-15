# ADR 0002 — Extension points are traits with registries, compiled in

- **Status:** Accepted
- **Date:** 2026-11-15

## Context

foobar2000 and fooyin are defined by extensibility. fooyin loads C++ shared libraries
implementing a handful of interfaces: input (decoder and tag reader), output, DSP,
encoder, and GUI widget. Most of its own features ship as such plugins.

Rust has no stable ABI: two crates compiled separately cannot safely share trait
objects across a dynamic-library boundary. Dynamic plugins in Rust therefore mean a C
ABI (losing the type system at the seam) or WebAssembly components (sandboxed, but a
large dependency and too slow for a decode or DSP inner loop today).

## Decision

Every point where fooyin has a plugin interface, aeri has a **trait and a registry**,
and every implementation is compiled into the binary.

- The traits are `Decoder`, `OutputSink`, `DspNode`, and the GUI's widget trait.
  Others are added when a second implementation exists, not before.
- A registry maps a stable string id to a constructor. Persisted configuration (a DSP
  chain, a layout) refers to implementations by id, never by Rust type.
- Optional implementations with heavy dependencies sit behind cargo features.
- There is no runtime plugin loading and no plugin ABI.

## Consequences

- aeri is composable for its developers, not extensible by its users. A third party
  adds a decoder by sending a pull request or keeping a fork.
- The core/plugin discipline is kept without the loader: a feature that could be a
  plugin in fooyin depends only on the public trait and the registry in aeri.
- String ids are a compatibility surface from the first release that persists them.
- Revisit when there is user demand for third-party extensions *and* a concrete
  extension that cannot reasonably be merged. The route then is WebAssembly components
  for non-real-time extension points, recorded as a new ADR.
