# Architecture decision records

An ADR records one decision, why it was made, and what it costs. The code shows *what*
was built; these files are the only place that says *why*, and why not the alternative.

## When an ADR is required

Write one **before** the code when a change does any of the following:

- adds, removes, or replaces a dependency that shapes a crate (not a patch bump);
- adds a crate, or changes which crate may depend on which;
- changes a "locked decision" or a pillar in [`ARCHITECTURE.md`](../ARCHITECTURE.md);
- changes a public trait that crosses a crate boundary (`Player`, `OutputSink`,
  `Decoder`);
- introduces `unsafe` somewhere new, a new thread, or a new channel between threads;
- relaxes a lint, a hook, or a `deny.toml` rule.

Not required for: bug fixes, tests, refactors inside one crate that keep its public
API, documentation, and dependency patch updates.

## Rules

1. **Never contradict an accepted ADR silently.** If code needs to go against one,
   write a new ADR that supersedes it, and get the user's agreement first. "It was
   easier" is not a reason; a fact the old ADR did not know is.
2. **ADRs are append-only.** Do not rewrite an accepted ADR's decision. To change
   course, add a new ADR and set the old one's status to `Superseded by ADR NNNN`.
3. **State what you verified and what you did not.** If a claim about a library or
   platform comes from memory, say so and say when it must be checked.
4. **Every ADR names its costs.** A decision with no downside listed has not been
   thought through.
5. **Update the index:** add the decision to the "Resolved" table in
   `ARCHITECTURE.md`, and remove it from "Open decisions" if it was listed.

## Template

Copy this into `NNNN-short-title.md`, numbered one past the highest existing ADR.

```markdown
# ADR NNNN — Title stated as the decision

- **Status:** Proposed | Accepted | Superseded by ADR NNNN
- **Date:** YYYY-MM-DD

## Context

What forces a decision now. The facts, the constraint, and the alternatives that were
really on the table.

## Decision

What we do, in the present tense. Specific enough that a reviewer can tell whether a
given change follows it.

## Consequences

What this costs, what it rules out, what must now be kept true, and what was not
verified. Include the condition under which this should be revisited.
```

## Index

| ADR | Decision |
| --- | --- |
| [0001](0001-multi-client-daemon.md) | The daemon is multi-client |
| [0002](0002-defer-wire-protocol.md) | Phase 3 ships `LocalPlayer` only; wire protocol in Phase 4 |
| [0003](0003-ipc-local-feature.md) | `aeri-ipc` depends on `aeri-engine` only behind `local` |
| [0004](0004-native-output-backends.md) | Native output backends; bit-perfect and gapless scoped |
| [0005](0005-sqlite-single-writer.md) | One SQLite writer, N WAL readers |
| [0006](0006-platform-integrations-are-player-clients.md) | Platform integrations are `Player` clients |
| [0007](0007-gpui-for-the-gui.md) | The GUI is built on GPUI |
| [0008](0008-format-floor-aac-alac.md) | AAC and ALAC in the format floor; Opus is a C exception |
| [0009](0009-defer-metadata-schema.md) | Metadata schema designed in Phase 4 |
| [0010](0010-engine-is-an-actor.md) | The engine is an actor; state fans out on a watch channel |
