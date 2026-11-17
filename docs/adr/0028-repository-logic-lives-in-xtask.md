# ADR 0028 — Logic that checks the repository lives in `xtask`, not in hook entries

- **Status:** Accepted
- **Date:** 2026-11-16

## Context

Five hook entries in `.pre-commit-config.yaml` held shell: `coverage`, `mutants`, `loom`, `fuzz`, and `crate-cooldown`. The last one fetched a Python script from GitHub at a SHA read out of `ci.yml` with `sed`, then ran it with `uv`. Logic in a YAML string cannot be unit tested, differs between shells, and is read by neither the compiler nor clippy. `xtask` already holds `check-deps`, `install-tools` and `install-hooks` in Rust.

The crate cooldown (seven days, [`TOOLING.md`](../TOOLING.md#pinning-and-cooldown)) needs each locked crate's publication time. The crates.io sparse index carries it as a `pubtime` field on every version's line (read for `fastrand` on 2026-11-16). Cargo 1.100.0 has no option that applies a cooldown itself.

Alternatives considered:

- **Keep Astral's script, move the shell into a script file.** Still runs code fetched at push time, needs `uv` and Python, and is a second implementation of a rule written in Rust elsewhere.
- **Vendor Astral's script.** Python in the repository for a rule about Rust crates, and copied code.
- **An HTTP client crate (`ureq`).** Brings a TLS stack and its build scripts into the tool that guards the supply chain.

## Decision

- A hook entry is one command, `cargo xtask <task>`, or one plain cargo command. Anything with a pipe, a redirect, a variable, `cd`, or a second command becomes an `xtask` task.
- New tasks: `coverage`, `mutants`, `loom`, `fuzz`, `crate-cooldown`. Their environment variables keep their names (`MUTANTS_BASE`, `FUZZ_SECONDS`).
- `crate-cooldown` reads `Cargo.lock` and `fuzz/Cargo.lock`, fetches each registry crate's index file with `curl`, and fails on any locked version published less than `cooldown-days` ago. The policy is `.config/crate-cooldown.toml`. A version with no `pubtime` is treated as old: the field was added to the index after most crates were published.
- The index is parsed with `serde_json`, a direct dependency of `xtask` only; it was already in `Cargo.lock` through `cargo_metadata`.
- CI runs `cargo xtask crate-cooldown` in the `crate cooldown` job. Astral's action and `.github/cargo-cooldowns.toml` are removed.

## Consequences

- The hook and CI run the same code, written in the language of the project, with unit tests for the parts that need no network: lockfile parsing, index paths, timestamps, and the age rule.
- We own the cooldown logic. A fault in it fails open, so its tests cover the boundary at exactly `cooldown-days`.
- The check needs `curl` and the network, as the hook did, and about 170 index requests, made in batches of sixteen. It no longer needs `uv`, Python, or Astral's published script.
- The old job reused publication times saved by the last green run on `main`; this one does not. Not measured: its run time in CI.
- Not verified: that a crate re-published under the same version is impossible. crates.io does not allow it, which is why the lockfile version identifies one `pubtime`.
- Revisit if cargo gains a native cooldown, or the index drops `pubtime`.
