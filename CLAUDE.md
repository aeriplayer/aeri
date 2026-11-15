# aeri

A foobar2000-class music player for macOS and Linux, modelled on
[fooyin](https://github.com/fooyin/fooyin): playlists, a library, one script language,
and an interface built from widgets. Apple Silicon and modern Linux only; Windows is
never considered. Rust throughout. The project is meant to show Rust at its best; the
owner is new to Rust, so explain language-specific choices when you make them.

fooyin is GPL-3.0-or-later and aeri is `MIT OR Apache-2.0`. Read fooyin to learn how a
problem is shaped; never translate its code (ADR 0001).

## Status

Phase 1 (CI, release pipeline, hooks) is complete. **Next: Phases 2–3, first audio**,
specified step by step in
[`docs/design/phase-2-3-playback.md`](docs/design/phase-2-3-playback.md).

Five crates exist: `aeri-core`, `aeri-audio`, `aeri-decode`, `aeri-engine`,
`aeri-cli`. All are empty apart from doc comments. Planned crates are created in the
phase that first needs them — do not scaffold early.

Update this section whenever a step or phase completes.

## Read before you write code

| Document | What it settles |
| --- | --- |
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | Pillars, crate graph, threading model, locked decisions, open decisions |
| [`docs/RUST_STYLE.md`](docs/RUST_STYLE.md) | How Rust is written here, with reasons |
| [`docs/TESTING.md`](docs/TESTING.md) | Which verification each kind of code requires, and the tools |
| [`docs/TOOLING.md`](docs/TOOLING.md) | Why the hooks, CI, lints, and dependency policy are configured as they are |
| [`docs/adr/`](docs/adr/README.md) | Every decision and why; when a new ADR is required |
| [`docs/design/`](docs/design/) | The step-by-step brief for the current phase |
| [`docs/ROADMAP.md`](docs/ROADMAP.md) | Phases and their checklists |

## Working protocol

1. **Start** by reading the Status section above, the current design brief, and the
   ADRs it cites. Run `git status` and `git log -5`.
2. **Work on a branch**, never `main`: one branch per phase (`phase-3`), merged before
   the next phase begins. One brief step per commit.
3. **Follow the brief.** Build the types and traits it specifies. Do not add features,
   abstractions, or crates it does not ask for.
4. **Verify, do not recall.** Before calling a third-party API or naming a version,
   read the docs for the version in `Cargo.lock` or the one `cargo add` resolves.
   Training memory of crate APIs is frequently out of date.
5. **Add a dependency only with the code that uses it.** Unused ones fail the build.
6. **Finish** only when `prek run --all-files` and
   `prek run --all-files --stage pre-push` both pass, new behaviour has tests, public
   items have rustdoc, and Status and the roadmap checklist are updated.
7. **Keep comments rare.** Write one only if deleting it would let a reader make a
   mistake they cannot see coming, in at most two lines (RUST_STYLE §12). Longer
   explanations go in rustdoc, `docs/`, or an ADR.
8. **Report honestly.** Say what was not verified, what was not run, and what you were
   unsure of. An unverified claim stated as fact is worse than a gap.

### Stop and ask the user before you

- weaken, skip, or suppress any check — a lint level, a hook, a `deny.toml` rule,
  `--no-verify`, or a crate-wide `#[expect]`. A failing check means the code changes;
- contradict an accepted ADR, a locked decision, or a pillar;
- add a dependency the brief does not name, or a new crate;
- write `unsafe` outside `aeri-audio`, or allocate, lock, log, or block in the
  real-time audio callback;
- change a public trait that crosses a crate boundary (`OutputSink`, `Decoder`,
  `DspNode`), or an existing method of the `Player` handle;
- push, merge to `main`, rewrite history, or tag a release.

When a decision of that kind is agreed, record it as an ADR before writing the code
(see [`docs/adr/README.md`](docs/adr/README.md)).

## Rules the tooling enforces

These fail the commit or the push; you do not need to remember them, only not to fight
them.

- No `unwrap()`, `todo!()`, `dbg!()`, or `println!` outside tests; no `unsafe` without
  opting in; every `unsafe` block has a `// SAFETY:` comment; public items are
  documented and `Debug`; suppressions are `#[expect(lint, reason = "…")]`, never
  `#[allow]`.
- Only the engine depends on `aeri-audio` and `aeri-decode`. Frontends reach the
  application through `aeri-player` once it exists; until then `aeri-cli` alone may
  depend on `aeri-engine` (`deny.toml`, ADR 0006).
- No unused dependencies; licences limited to the `deny.toml` allowlist.
- Conventional Commits, scoped by crate, imperative subject of at most 72 characters,
  type one of `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `build`, `ci`,
  `chore`, `revert`. Commits are signed. No AI attribution anywhere in a commit.
- No commits on `main`.

```text
feat(engine): preload next track 5s before end of current
fix(decode): skip corrupt packets instead of ending the track
docs(adr): record ring buffer sizing rationale
```

## Commands

```bash
# Once per clone
brew install prek
cargo xtask install-hooks
cargo xtask install-tools   # nextest, cargo-hack, llvm-cov, cargo-mutants

# Commit-stage checks over the whole tree (hygiene, fmt, lints, clippy)
prek run --all-files

# Push-stage checks (tests, docs, dependency policy)
prek run --all-files --stage pre-push

# One hook
prek run cargo-clippy --all-files

# On-demand checks that CI runs on every pull request
prek run coverage --stage manual --all-files
prek run mutants --stage manual --all-files
prek run miri --stage manual --all-files   # needs a nightly toolchain with miri

# Move pinned hooks to their latest releases (commit SHAs, 7-day cooldown)
prek update
```

Formatters rewrite files and fail the commit; re-stage and commit again.

## Toolchain

- Edition 2024. `rust-toolchain.toml` pins an exact stable so CI cannot break on an
  unrelated release. Bump it in its own commit with every check green.
- `rustfmt.toml` is defaults at edition 2024. Import grouping is a review convention:
  the rustfmt options for it are nightly-only.
- Hook tools written in Rust build with prek's own newer toolchain, not the pinned one.

## Testing

- Unit tests in `#[cfg(test)] mod tests` beside the code; cross-module behaviour in each
  crate's `tests/`.
- `aeri-engine` is tested against `FakeSink`, never a real device. Device sinks are
  verified by hand and by unit tests of the callback body.
- Fixtures are small committed files with the command that generated them beside them.

## Distribution

`cargo-dist` drives releases from `dist-workspace.toml`: signed and notarized `.dmg` on
macOS, an AUR `PKGBUILD` in `pkg/aur/`, a Homebrew formula in `pkg/homebrew/`. Nothing
has been released yet and the release workflow has never run. Licence:
`MIT OR Apache-2.0`, declared by every crate.

<!-- code-review-graph MCP tools -->
## MCP Tools: code-review-graph

**IMPORTANT: This project has a knowledge graph. ALWAYS use the
code-review-graph MCP tools BEFORE using Grep/Glob/Read to explore
the codebase.** The graph is faster, cheaper (fewer tokens), and gives
you structural context (callers, dependents, test coverage) that file
scanning cannot.

### When to use graph tools FIRST

- **Exploring code**: `semantic_search_nodes` or `query_graph` instead of Grep
- **Understanding impact**: `get_impact_radius` instead of manually tracing imports
- **Code review**: `detect_changes` + `get_review_context` instead of reading entire files
- **Finding relationships**: `query_graph` with callers_of/callees_of/imports_of/tests_for
- **Architecture questions**: `get_architecture_overview` + `list_communities`

Fall back to Grep/Glob/Read **only** when the graph doesn't cover what you need.

### Key Tools

| Tool | Use when |
| ------ | ---------- |
| `detect_changes` | Reviewing code changes — gives risk-scored analysis |
| `get_review_context` | Need source snippets for review — token-efficient |
| `get_impact_radius` | Understanding blast radius of a change |
| `get_affected_flows` | Finding which execution paths are impacted |
| `query_graph` | Tracing callers, callees, imports, tests, dependencies |
| `semantic_search_nodes` | Finding functions/classes by name or keyword |
| `get_architecture_overview` | Understanding high-level codebase structure |
| `refactor_tool` | Planning renames, finding dead code |

### Workflow

1. The graph auto-updates on file changes (via hooks).
2. Use `detect_changes` for code review.
3. Use `get_affected_flows` to understand impact.
4. Use `query_graph` pattern="tests_for" to check coverage.
