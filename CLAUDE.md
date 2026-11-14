# aeri

Modern audio player for macOS and Linux. Apple Silicon and modern Linux only. Rust
throughout, with one C codec (libopus).

This document is the source of truth for **project conventions, build commands, and
current status**. System design and structural decisions live in
[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — read it before writing any code.

## Current status

Phase 1 (CI + cargo-dist) complete. Next: Phase 2 (core playback types), which has no
blockers and leads straight into Phase 3 (first audio).

Decisions are recorded as ADRs in `docs/adr/` (0001–0009). The metadata schema is
deliberately deferred to Phase 4.

Six crates exist today: `aeri-core`, `aeri-audio`, `aeri-decode`, `aeri-engine`,
`aeri-ipc`, `aeri-cli`. Crates marked *planned* below are created in the phase that
first needs them — do not scaffold them early.

See [`docs/ROADMAP.md`](docs/ROADMAP.md) for the full phase breakdown.

## Workspace layout

```text
aeri/
├── Cargo.toml                  # workspace root, [workspace.dependencies], [workspace.lints]
├── rust-toolchain.toml         # latest stable
├── rustfmt.toml                # minimal
├── prek.toml                   # Git hooks; the single definition of "passes checks"
├── deny.toml                   # cargo-deny: licences, advisories, sources
├── committed.toml              # commit message policy
├── taplo.toml · .rumdl.toml    # TOML and Markdown formatting
├── dist-workspace.toml         # cargo-dist
├── LICENSE-MIT
├── LICENSE-APACHE
├── COPYRIGHT
├── proto/                      # .proto files for IPC
│   └── aeri/v1/player.proto
├── crates/
│   ├── aeri-core/              # domain types: Track, PlaybackState, Queue (schema in Phase 4)
│   ├── aeri-audio/             # OutputSink, native CoreAudio + PipeWire sinks, RT thread
│   ├── aeri-decode/            # symphonia adapter, format trait
│   ├── aeri-dsp/               # (planned, Phase 5) DSP nodes: replay gain, EQ, resampler
│   ├── aeri-library/           # (planned, Phase 4) rusqlite + FTS5, lofty, scanning
│   ├── aeri-engine/            # composes audio + decode + dsp + library
│   ├── aeri-ipc/               # connectrpc service, Player trait, Local + Remote impls
│   ├── aeri-platform/          # (planned, Phase 5) MPRIS / MediaPlayer; a Player client
│   ├── aeri-stream/            # (planned, post-v1) HTTP audio streaming + Subsonic API
│   ├── aeri-cli/               # `aeri` binary; also runs as daemon
│   └── aeri-gui/               # (planned, Phase 6) GPUI GUI binary
├── xtask/                      # build/release automation
├── pkg/
│   ├── aur/                    # PKGBUILD
│   └── homebrew/               # Formula
└── docs/                       # ARCHITECTURE.md, ROADMAP.md, adr/
```

## Conventions

### Rust

- Edition 2024
- `rust-toolchain.toml` pins an exact stable (currently `1.95.0`), so CI cannot break
  on an unrelated Rust release; bump it in its own commit with clippy verified green
- Workspace `[lints.clippy]` enables `pedantic` and `nursery` at warn; targeted
  overrides for noisy lints documented in the workspace `Cargo.toml`
- `rustfmt.toml`: defaults, pinned to `edition`/`style_edition` 2024. Import granularity
  and grouping are *not* configured — `imports_granularity` and `group_imports` are
  nightly-only rustfmt options and are silently ignored on the pinned stable toolchain,
  so import layout is a review convention rather than an enforced one
- `unsafe` is forbidden outside `aeri-audio` (where FFI and real-time bridges live).
  Every `unsafe` block requires a `// SAFETY:` comment explaining the invariant.
- `unwrap()` is forbidden outside tests, `examples/`, and `xtask/`. Use `expect()` with
  a justification or propagate errors properly.
- Logging is `tracing` only. No `println!` outside CLI user-facing output and tests.

### Branching

- **Never commit directly to `main`.**
- Each phase gets its own branch: `phase-2`, `phase-3`, etc.
- All work for a phase is committed to that branch.
- A phase branch is merged to `main` before the next phase begins.

### Commits

Conventional Commits, scoped by crate. Enforced at commit time (`committed.toml`):
imperative subject of at most 72 characters, no trailing full stop, type one of `feat`,
`fix`, `perf`, `refactor`, `docs`, `test`, `build`, `ci`, `chore`, `revert`. Commits are
signed and carry no AI-attribution trailers.

```text
feat(engine): preload next track 5s before end of current
fix(library): handle UTF-8 BOM in M3U playlists
chore(workspace): bump tokio to 1.46
docs(architecture): record ring buffer sizing rationale
```

### Documentation

- Public items in library crates require rustdoc
- `cargo doc --no-deps --document-private-items` should be warning-free
- Non-obvious architectural decisions go in `docs/adr/` as ADRs

## Testing

- Unit tests inside each crate's `src/`
- Integration tests in each crate's `tests/`, especially `aeri-library` (round-trip
  lofty reads/writes against fixture files) and `aeri-engine` (pipeline behavior against
  a fake audio sink)
- Benchmark suite in `benches/` for hot paths: library scan, FTS query, DSP nodes,
  decoder throughput
- The library crate should have a fixture set of small FLAC/MP3/Ogg files in
  `tests/fixtures/` for round-trip metadata testing

The integration test suite is the source of confidence for the library crate. Coverage
of every query path is expected.

## First-time setup

```bash
# Install prek (https://prek.j178.dev), then the Git hooks
brew install prek
cargo xtask install-hooks
```

Run this once after cloning.

## Checks

Every check is defined once, in `prek.toml`, and CI runs that file rather than
restating commands. Hooks run at three points:

| Stage | What runs |
| --- | --- |
| `commit-msg` | Conventional Commits (`committed.toml`); no AI-attribution trailers |
| `pre-commit` | Hygiene, `cargo fmt`, typos, TOML and Markdown formatting, workflow lint (actionlint, zizmor), unused dependencies (cargo-machete), `cargo clippy -D warnings`; refuses commits on `main` |
| `pre-push` | `cargo test`, `cargo doc -D warnings`, `cargo deny`, signed-commit check |

```bash
# Commit-stage hooks over the whole tree
prek run --all-files

# Push-stage hooks (tests, docs, dependency policy)
prek run --all-files --stage pre-push

# One hook
prek run cargo-clippy --all-files

# Move every pinned hook to its latest release (pins are commit SHAs, 7-day cooldown)
prek update
```

Formatters rewrite files and fail the commit; re-stage and commit again. Never bypass
hooks with `--no-verify`. A dependency that nothing uses fails `cargo-machete`, so add
dependencies with the code that needs them, not ahead of it.

## Build and run

```bash
# Check everything
cargo check --workspace --all-targets

# Run unit + integration tests
cargo test --workspace

# Run the GUI (from Phase 6)
cargo run -p aeri-gui

# Run the daemon
cargo run -p aeri-cli -- daemon

# CLI control
cargo run -p aeri-cli -- play /path/to/track.flac
cargo run -p aeri-cli -- queue list
cargo run -p aeri-cli -- pause

# Lint — runs on every commit via prek; this is the same command
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings

# Format
cargo fmt --all

# Generate IPC code (run after editing .proto files; requires protoc)
cargo xtask gen-proto

# Release build with signing/notarization
cargo xtask release
```

## Distribution

- **macOS**: Developer ID Application signed, notarized via `notarytool`, stapled.
  Distributed as `.dmg`. Apple Developer Program enrollment required ($99/year).
- **Linux Arch**: `PKGBUILD` lives in `pkg/aur/`, published to AUR.
- **macOS Homebrew**: `Formula` lives in `pkg/homebrew/`, published to a personal tap
  initially, `homebrew-core` once notability thresholds are met.
- **All releases**: Sigstore signed, GitHub build provenance attestations attached.

`cargo-dist` drives everything from `dist-workspace.toml`. CI secrets needed: Apple
Developer ID cert, app-specific password, Homebrew GitHub token.

## License

Dual-licensed: `MIT OR Apache-2.0`. Every crate's `Cargo.toml` declares:

```toml
license = "MIT OR Apache-2.0"
```

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
