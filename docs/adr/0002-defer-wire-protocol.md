# ADR 0002 — Phase 3 ships `LocalPlayer` only; the wire protocol waits for Phase 4

- **Status:** Accepted
- **Date:** 2026-11-14

## Context

The composability pillar is bought by the `Player` trait: frontends program against it
and never touch the engine. Connect-RPC and protobuf are one transport behind that
trait. Phase 3 originally required both the trait and a first set of RPCs, which puts
protocol design ahead of the first working audio path and ahead of knowing what the
trait needs to say.

## Decision

Phase 3 delivers the `Player` trait and `LocalPlayer` only. The Connect service,
`RemotePlayer`, and the daemon are built at the start of Phase 4, before the library
RPCs that need them.

Connect-RPC over gRPC remains the chosen transport; this ADR changes when, not what.

## Consequences

- Phase 3's CLI is `aeri play <path>` running in the foreground. `pause`, `stop`, and
  `seek` as separate invocations need a daemon to talk to, so they move to Phase 4.
- `proto/aeri/v1/player.proto` stays an empty service and the `aeri-ipc` build script
  keeps generating from it, so the codegen path stays exercised in CI.
- The proto is designed against a trait that has already carried real playback, rather
  than the other way round.
- Risk: the trait could drift toward shapes that do not serialise well (borrowed data,
  callbacks). Mitigation: trait methods take and return owned `aeri-core` types only.
