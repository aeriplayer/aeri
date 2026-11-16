# Architecture decision records

An ADR records one decision, why it was made, and what it costs. The code shows *what* was built; these files are the only place that says *why*, and why not the alternative.

## When an ADR is required

Write one **before** the code when a change does any of the following:

- adds, removes, or replaces a dependency that shapes a crate (not a patch bump);
- adds a crate, or changes which crate may depend on which;
- changes a "locked decision" or a pillar in [`ARCHITECTURE.md`](../ARCHITECTURE.md);
- changes a public trait that crosses a crate boundary (`OutputSink`, `Decoder`, `DspNode`), or the `Player` handle's existing methods;
- introduces `unsafe` somewhere new, a new thread, or a new channel between threads;
- relaxes a lint, a hook, or a `deny.toml` rule.

Not required for: bug fixes, tests, refactors inside one crate that keep its public API, documentation, and dependency patch updates.

## Rules

1. **Never contradict an accepted ADR silently.** If code needs to go against one, write a new ADR that supersedes it, and get the user's agreement first. "It was easier" is not a reason; a fact the old ADR did not know is.
2. **ADRs are append-only.** Do not rewrite an accepted ADR's decision. To change course, add a new ADR and set the old one's status to `Superseded by ADR NNNN`.
3. **State what you verified and what you did not.** If a claim about a library or platform comes from memory, say so and say when it must be checked.
4. **Every ADR names its costs.** A decision with no downside listed has not been thought through.
5. **Update the index:** add the decision to the "Resolved" table in `ARCHITECTURE.md`, and remove it from "Open decisions" if it was listed.

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
| [0001](0001-desktop-player-scope.md) | A foobar2000-class desktop player; no daemon, no network API |
| [0002](0002-compiled-in-extension-points.md) | Extension points are traits with registries, compiled in |
| [0003](0003-native-output-backends.md) | Native output backends; bit-perfect and gapless scoped |
| [0004](0004-engine-is-an-actor.md) | The engine is an actor; state fans out on a watch channel |
| [0005](0005-engine-plays-current-and-next.md) | The engine plays a current track and a next slot |
| [0006](0006-frontends-use-the-player-handle.md) | Frontends use one concrete `Player` handle |
| [0007](0007-symphonia-flac-mp3-first.md) | symphonia, FLAC and MP3 first; formats on demand |
| [0008](0008-library-sqlite-and-in-memory-store.md) | SQLite for persistence, an in-memory store for queries |
| [0009](0009-open-tag-metadata.md) | Open, multi-value tag map; schema waits for the library |
| [0010](0010-one-script-language.md) | One script language formats and queries tracks |
| [0011](0011-gpui-and-layout-as-data.md) | The GUI is built on GPUI; layout is data |
| [0012](0012-filesystem-through-fs-err.md) | Filesystem access goes through fs-err |
| [0013](0013-property-tests-with-proptest.md) | Property tests use proptest |
