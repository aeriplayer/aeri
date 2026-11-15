# ADR 0001 — aeri is a foobar2000-class desktop player; no daemon, no network API

- **Status:** Accepted
- **Date:** 2026-11-15

## Context

aeri is a modern foobar2000: open source, in Rust, native on Linux and macOS.

[fooyin](https://github.com/fooyin/fooyin) is an open-source player of exactly that kind (C++ and Qt6, about 400,000 lines at 0.15.0). It shows what the product is when finished: playlist-centric playback, a library, one script language for display and queries, and an interface assembled from widgets in an editable layout.

A music player can also grow into a server: a headless daemon, a network API, streaming to other devices, mobile clients. Each of those shapes every layer beneath it, so the scope has to be stated before the layers are built.

## Decision

aeri is a single-process desktop music player modelled on foobar2000 and fooyin.

- **One process.** There is no daemon, no wire protocol, no network API, no streaming to other devices, and no mobile client. A proposal for any of them starts from a new ADR.
- **Platforms:** Apple Silicon macOS and modern Linux. Windows is never considered.
- **Licence:** `MIT OR Apache-2.0`. fooyin is GPL-3.0-or-later, so aeri takes its features and structure from fooyin and its code from neither fooyin nor any other project with an incompatible licence.

## Consequences

- Remote control of a running player is not designed. On Linux, MPRIS provides it for free; a general answer is an open decision.
- Nothing can be ported from fooyin, only re-derived, which costs speed.
- fooyin's size is the honest estimate of the destination. The roadmap orders work so that each phase is a usable player, not a fraction of a large one.
