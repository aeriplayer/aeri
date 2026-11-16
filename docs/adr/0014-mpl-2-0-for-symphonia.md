# ADR 0014 — MPL-2.0 is allowed for symphonia's crates only

- **Status:** Accepted
- **Date:** 2026-11-15

## Context

[ADR 0007](0007-symphonia-flac-mp3-first.md) makes symphonia the decoder. Every symphonia crate is licensed MPL-2.0, which `deny.toml` does not allow: its allowlist is MIT, Apache-2.0, Unicode-3.0 and Zlib. ADR 0007 did not consider the licence.

MPL-2.0 is copyleft at the level of the file. Read from the licence text, not from legal advice:

- aeri's own code stays `MIT OR Apache-2.0`. Combining it with MPL files in one binary is permitted (MPL-2.0 §3.3).
- Changes to symphonia's own files must be published under MPL-2.0.
- A distributed binary must tell recipients how to obtain the source of the MPL-covered files, and must carry the MPL-2.0 notice (§3.2).

The alternatives were adding MPL-2.0 to the general allowlist, or replacing symphonia with a decoder stack under permissive licences. A global allowance would let any future MPL crate in without review. Replacing symphonia would reopen ADR 0007 before a single file has played.

## Decision

- `deny.toml` gains a `[licenses] exceptions` entry allowing MPL-2.0 for each symphonia crate by name: `symphonia`, `symphonia-bundle-flac`, `symphonia-bundle-mp3`, `symphonia-common`, `symphonia-core`, `symphonia-metadata`. MPL-2.0 is not on the general allowlist.
- A format added under ADR 0007 that pulls in another symphonia crate adds that crate to the exception in the same change.
- aeri does not modify symphonia's files. A needed fix goes upstream, or the change carries a new ADR.

## Consequences

- Any MPL crate outside symphonia still fails `cargo deny`, so each one is a deliberate decision.
- **Release obligation, not yet met.** Before the first release, every distributed artefact (the `.dmg`, the AUR package, the Homebrew formula) must ship the third-party licence notices and point to symphonia's source. Nothing in the release pipeline does this yet, and no release may be cut until it does. Generating a notices file at release time (for example with `cargo-about`) is the expected route; that tool is unevaluated.
- The exception list must be kept in step with the crates that symphonia's enabled features actually pull in. `cargo deny` fails if one is missing, and warns if one is stale.
- Revisit if symphonia changes its licence, or if legal review of the distribution obligations reaches a different reading than the one above.
