# ADR 0003 — `aeri-ipc` depends on `aeri-engine` only behind a `local` feature

- **Status:** Accepted
- **Date:** 2026-11-14

## Context

`aeri-ipc` holds the `Player` trait, `LocalPlayer`, and `RemotePlayer`. `LocalPlayer`
owns an engine, so `aeri-ipc` depended on `aeri-engine` unconditionally. Every consumer
of the trait — including a thin remote-only client — therefore linked cpal, symphonia,
and SQLite.

## Decision

`aeri-engine` is an optional dependency of `aeri-ipc`, enabled by a `local` feature
that is on by default. `LocalPlayer` is compiled only with `local`. The `Player` trait
and `RemotePlayer` are always available.

`aeri-cli` and `aeri-gui` use the default and so get the single-binary build. A
remote-only client depends on `aeri-ipc` with `default-features = false`.

## Consequences

- `cargo check -p aeri-ipc --no-default-features` must stay green; nothing outside the
  `local`-gated module may name `aeri_engine`.
- Splitting the trait into its own crate was considered and rejected for now: a feature
  achieves the same link-time result without a twelfth crate.
