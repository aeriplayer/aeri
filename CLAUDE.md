# aeri

Modern audio player for macOS and Linux. Apple Silicon and modern Linux only. Pure Rust.

This document is the source of truth for **project conventions, build commands, and
current status**. System design and structural decisions live in
[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — read it before writing any code.

## Current status

Phase 1 (CI + cargo-dist) complete. Next: Phase 2 (core domain types).

Resolve metadata schema and daemon concurrency model before writing any Phase 2 code.

See [`docs/ROADMAP.md`](docs/ROADMAP.md) for the full phase breakdown.

## Workspace layout

```text
aeri/
├── Cargo.toml                  # workspace root, [workspace.dependencies], [workspace.lints]
├── rust-toolchain.toml         # latest stable
├── rustfmt.toml                # minimal
├── dist-workspace.toml         # cargo-dist
├── LICENSE-MIT
├── LICENSE-APACHE
├── COPYRIGHT
├── proto/                      # .proto files for IPC
│   └── aeri/v1/player.proto
├── crates/
│   ├── aeri-core/              # domain types: Track, Album, PlaybackState, Queue
│   ├── aeri-audio/             # cpal output, PipeWire + CoreAudio backends, RT thread
│   ├── aeri-decode/            # symphonia adapter, format trait
│   ├── aeri-dsp/               # DSP nodes: replay gain, EQ, resampler integration
│   ├── aeri-library/           # rusqlite + FTS5, lofty, scanning, filesystem watch
│   ├── aeri-engine/            # composes audio + decode + dsp + library
│   ├── aeri-ipc/               # connectrpc service, Player trait, Local + Remote impls
│   ├── aeri-platform/          # MPRIS (Linux), MediaPlayer framework (macOS)
│   ├── aeri-stream/            # (planned, post-v1) HTTP audio streaming + Subsonic API
│   ├── aeri-cli/               # `aeri` binary; also runs as daemon
│   └── aeri-gui/               # iced GUI binary
├── xtask/                      # build/release automation
├── pkg/
│   ├── aur/                    # PKGBUILD
│   └── homebrew/               # Formula
└── docs/                       # ARCHITECTURE.md, ROADMAP.md, ADRs
```

## Conventions

### Rust

- Edition 2024
- `rust-toolchain.toml` pins latest stable; bump as new stable releases ship
- Workspace `[lints.clippy]` enables `pedantic` and `nursery` at warn; targeted
  overrides for noisy lints documented in the workspace `Cargo.toml`
- `rustfmt.toml`: defaults plus `imports_granularity = "Module"` and
  `group_imports = "StdExternalCrate"`
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

Conventional Commits, scoped by crate:

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
# Install the pre-commit hook (fmt + clippy — mirrors CI)
cargo xtask install-hooks
```

Run this once after cloning. The hook blocks commits that would fail CI.

## Build and run

```bash
# Check everything
cargo check --workspace --all-targets

# Run unit + integration tests
cargo test --workspace

# Run the GUI
cargo run -p aeri-gui

# Run the daemon
cargo run -p aeri-cli -- daemon

# CLI control
cargo run -p aeri-cli -- play /path/to/track.flac
cargo run -p aeri-cli -- queue list
cargo run -p aeri-cli -- pause

# Lint — must pass clean before every commit
cargo clippy --workspace --all-targets -- -D warnings

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
