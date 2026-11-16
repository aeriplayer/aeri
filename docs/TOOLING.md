# Tooling rationale

Why the repository's configuration files are shaped the way they are. The files themselves carry no prose beyond a line where a reader would otherwise "fix" something correct (see [`RUST_STYLE.md` §12](RUST_STYLE.md#12-comments)). Commands are in [`README.md`](../README.md); which verification each kind of code needs is in
[`TESTING.md`](TESTING.md).

## One definition of "passes checks"

Every check is defined once, in `.pre-commit-config.yaml`. The file is YAML, not `prek.toml`, because Renovate's pre-commit manager reads that format and updates its `# frozen:` pins natively; prek's extensions (priorities, groups, `builtin` hooks, `update`) work in either format. CI runs the same hooks rather than restating their commands, so a green hook and a green CI cannot disagree. CI only decides *where* each group runs:

- **Commit-msg stage:** the message itself (Conventional Commits, forbidden trailers).
- **Pre-commit stage:** fast, on staged files: hygiene, formatting, lints, clippy.
- **Pre-push stage:** slow, whole workspace: tests, docs, feature powerset, dependency policy.
- **Manual stage:** coverage, mutation testing, Miri. Too slow for every push; CI runs them on every pull request.

Hooks that only make sense on a developer machine (signed commits, no commits to `main`) are skipped in CI: CI builds `main` itself, and merge commits created by GitHub are not signed with a contributor key.

Two checks exist only in CI, because they need GitHub itself: the crate cooldown and the release plan (below).

### Priorities

Hooks run in ascending priority; hooks sharing a priority run concurrently. Anything that rewrites files has a priority of its own, because two fixers touching one file at once is undefined. Cargo hooks are serialised as well: they would only queue on the build-directory lock.

### The `target` group

`language = "system"` runs the cargo that `rust-toolchain.toml` selects, so hooks, CI and `cargo build` use one compiler. Hooks in the `target` group (clippy, tests, docs, feature powerset) give different answers per platform because the audio backends are `cfg(target_os)`. CI runs that group on every supported platform and everything else once.

### Pinning and cooldown

Hooks and actions are pinned to commit SHAs, because a tag can be moved and a SHA cannot. Nothing published in the last seven days is adopted, so a compromised release has time to be noticed. Renovate applies that cooldown to the updates it proposes. The `crate cooldown` CI job applies it to `Cargo.lock` itself, so a hand-run `cargo update` or `cargo add` is held to it as well. It uses Astral's reusable `cargo-cooldowns` action (Apache-2.0), which reads publication times from the crates.io index. To take a release sooner, for a security fix, add `.github/cargo-cooldowns.toml` with an exclusion naming the crate, the version, and the reason.

Lockfile changes are made with `cargo update --precise <crate>@<version>`, never a bare `cargo update`, so a pull request changes only what it says it changes.

The `# frozen: vX` and `# vX` markers beside SHAs are data: Renovate reads them to know which release a SHA is. `prek update` writes the same format and remains usable by hand.

### What CI runs, and for how long

- **Planning.** A `plan` job compares a pull request with its base. Changes confined to `docs/`, Markdown, `.claude/`, licences and `.rumdl.toml` skip the target matrix, coverage, mutants and Miri; the `checks` job still runs every hook over them. A crate that includes a Markdown file into its rustdoc with `include_str!` makes that file code, and the exclusions must then be narrowed. Pushes to `main` run everything. Skipped jobs count as passing for branch protection.
- **Timeouts.** Every job has `timeout-minutes`. GitHub's default is six hours. The nextest timeout covers a hung test, not a hung build, a tool download or a Miri run.
- **Retries.** `CARGO_NET_RETRY` and `RUSTUP_MAX_RETRIES` absorb transient registry and toolchain download failures.
- **Cache.** `Swatinem/rust-cache` keeps the registry and compiled dependencies per job kind. Only runs on `main` write it, so a pull request can read the cache but cannot poison it.
- **Concurrency.** A new push cancels the older run for the same pull request. A re-run of an older commit gets its own group, so it cannot cancel the latest one.
- **Release plan.** When the release inputs change (`dist-workspace.toml`, `release.yml`, manifests, the lockfile, the toolchain), `dist plan` runs, so a broken release configuration fails on the pull request rather than on the first tag.

### Deliberate gaps

- `check-hooks-apply` (prek's meta hook) is omitted: several hooks guard things the repository does not contain yet, such as symlinks.
- `LICENSE-*` files are excluded from every hook so they stay byte-for-byte as published.

## Hooks that need explaining

| Hook | Why it exists in this form |
| --- | --- |
| `cargo-nextest` | One process per test, hung tests killed, no retries (`.config/nextest.toml`). A run that finds no tests fails. |
| `cargo-doctest` | nextest does not run documentation examples. |
| `cargo-hack` | Every feature combination must compile alone, so a gated boundary such as `aeri-audio`'s `test-util` feature cannot rot unnoticed. |
| `coverage` | Runs the tests once, then reports per floor: the workspace, and each pure-logic crate at 100% (`--package`). A crate with no code fails a 100% report, so each crate's line is added with its first code. Excludes `xtask/` and `build.rs`: build automation, not product code. The floor is a tripwire, not a goal ([`TESTING.md`](TESTING.md)). |
| `mutants` | Mutates only the lines that differ from the base branch, then requires the tests to notice every mutation. Excluded mutants are named in `.cargo/mutants.toml` with the reason. |
| `miri` | Needs `rustup toolchain install nightly --component miri`. Tests that cannot run under Miri (FFI, real I/O) opt out with `#[cfg_attr(miri, ignore)]`. |
| `cargo-deny` | Runs at push because it reads the advisory database over the network. |
| `cargo-shear` | Finds unused and misplaced dependencies from source references, across every `cfg` at once, including dev-dependencies, which Cargo's own `unused_dependencies` lint does not check. A false positive goes in `[package.metadata.cargo-shear] ignored` with a reason beside it. |
| `check-deps` | `cargo xtask check-deps`: the dependency rules cargo-deny cannot express (below). |

## Test runner policy

`.config/nextest.toml` encodes three decisions:

- Run everything and report every failure rather than stopping at the first.
- A hung test is a failure, not a stuck terminal. The engine tests join threads; if a join never returns, the timeout ends the run.
- No retries. A test that passes on the second attempt is flaky, and a flaky test is a bug in the test or in the code.

The `ci` profile applies the same rules and keeps output from passing tests out of the log.

## Dependency policy (`deny.toml`)

The mechanical half of the "defensible dependencies" pillar in
[`ARCHITECTURE.md`](ARCHITECTURE.md).

- **Targets** are only the platforms aeri ships on; a Windows-only advisory is not ours to chase.
- **Licences** are permissive ones compatible with shipping `MIT OR Apache-2.0` binaries.
- **Bans** enforce the crate graph: a listed crate may only be depended on directly by its `wrappers`. Only the engine composes the audio and decode layers. Frontends reach the application through `aeri-player` once it exists; until then `aeri-cli` is the engine's one permitted dependant (ADR 0006).
- **Build scripts** run arbitrary code at compile time, so only crates named in `[bans.build] allow-build-scripts` may have one. A new entry is a deliberate act in review. `check-deps` fails on entries that no longer match a crate, so the list cannot accumulate stale permissions.
- **Features are explicit.** Every external dependency a workspace crate declares sets `default-features = false` and names the features it uses (`check-deps`). Defaults are the crate author's guess at a typical user; this way nothing is compiled that our code did not ask for, and a new default in an upstream release does not arrive unannounced.
- `publish = false` on every crate keeps them off crates.io (aeri ships as binaries) and lets cargo-deny tell the workspace's own path dependencies from wildcard requirements.

## Lint policy (`Cargo.toml`)

Every rule in `README.md` that a compiler can check is checked in `[workspace.lints]`, and CI runs with `-D warnings`, so each `warn` is a hard failure. Suppress at one site with `#[expect(lint, reason = "…")]`, never `#[allow]`: `expect` fails once the code stops triggering the lint, so suppressions cannot go stale
([`RUST_STYLE.md` §10](RUST_STYLE.md#10-suppressing-a-lint)).

| Group | Intent |
| --- | --- |
| `unsafe_code`, `unsafe_op_in_unsafe_fn`, `undocumented_unsafe_blocks`, `multiple_unsafe_ops_per_block` | `unsafe` is denied everywhere. `aeri-audio` opts in once at its crate root when its first FFI call is written. Every block states its invariant and holds one operation. |
| `missing_docs`, `missing_debug_implementations`, `unreachable_pub`, `unnameable_types` | Public means public API, documented and printable. Anything else is `pub(crate)` or private. |
| `unwrap_used`, `get_unwrap`, `todo`, `unimplemented`, `dbg_macro`, `print_stdout`, `print_stderr` | No panicking shortcuts or stray output outside tests (`clippy.toml` exempts test code). `expect("why this cannot fail")` stays allowed. Logging is `tracing`; binaries opt in to printing where output is the product. |
| `allow_attributes*`, `unfulfilled_lint_expectations` | Suppressions are `#[expect]` with a reason, and a stale one is an error. |
| `clone_on_ref_ptr` | Shared ownership is spelled `Arc::clone(&x)`, so a pointer copy is never mistaken for a deep clone. |
| `lossy_float_literal` | A float literal that cannot be represented exactly is a bug in audio code. |
| `mem_forget`, `error_impl_error`, `tests_outside_test_module`, `mod_module_files`, `unused_trait_names` | Leaking is never the fix; error types are named for what failed; tests live in `tests`; `foo.rs` plus `foo/`, never `foo/mod.rs`; trait-only imports are `use Trait as _`. |
| `exit`, `empty_drop`, `empty_structs_with_brackets`, `rc_buffer`, `rc_mutex`, `rest_pat_in_fully_bound_structs` | No process exit outside `main`'s return, no meaningless `Drop` or `{}` structs, no `Rc` shapes that are almost always a mistake, no `..` that hides a field a reader needs to see. |
| `iter_over_hash_type` | Iterating a `HashMap` gives a different order on every run. Output that a user or a test sees must be deterministic, so iterate a `BTreeMap` or sort first. |

`clippy.toml` bans what a lint cannot express: `std::thread::spawn`, because long-lived threads are named (RUST_STYLE §5), and the `std::fs` functions and types, because file access goes through `fs_err` ([ADR 0012](adr/0012-filesystem-through-fs-err.md)). Each ban's message says what to use instead. Clippy reads the nearest `clippy.toml` *instead of* the root one, not merged with it: a per-crate file must repeat these entries.

`pedantic` and `nursery` sit at priority −1 so the individual levels win. `missing_const_for_fn` is allowed: it fires on functions that are incidentally const-able today but would become breaking changes to un-const later. `redundant_pub_crate` is allowed because it contradicts `unreachable_pub`: inside a private module one rejects `pub(crate)` and the other rejects `pub`, so with both on an item used by a sibling module has no legal visibility. `unreachable_pub` is the one the visibility rule above depends on.

`clippy.toml` sets `too-many-arguments-threshold = 5`: beyond five positional arguments, use a struct.

## Build profiles

| Profile | Settings | Why |
| --- | --- | --- |
| `dev` | `debug = "line-tables-only"` | Backtraces keep file and line; dropping type and variable debug information makes builds markedly faster (uv measured 20%). The cost is that a debugger shows no variable values: set `CARGO_PROFILE_DEV_DEBUG=full` for a debugging session. |
| `dev`, dependencies | `opt-level = 2` for every non-workspace crate | Decoding and DSP run in third-party code. Unoptimised, they can run slower than real time and make a debug build stutter for reasons that are not ours. Dependencies are compiled once and cached, so the cost is a slower first build. Our own code stays unoptimised and debuggable. |
| `release` | `lto = "fat"`, `codegen-units = 1`, `strip = true` | Smallest, fastest binary; the slow link is paid in CI, not in the edit loop. |
| `profiling` | `release` with `debug = "full"`, no strip, no LTO | Real symbols for samply or Instruments without waiting on LTO. Benchmarks build with it. |
| `dist` | inherits `release` | The profile `dist build` uses. |

Panics unwind in every profile. Whether a release build should abort on panic is undecided; it affects how the engine reports a crashed thread and needs an ADR.

## Toolchain

`rust-toolchain.toml` pins an exact stable so CI cannot break on an unrelated release. Bump it in its own commit with every check green. `llvm-tools` is there for `cargo-llvm-cov`.

The minimum macOS version is `MACOSX_DEPLOYMENT_TARGET` in `.cargo/config.toml`, which cargo passes to every compile and link. The macOS CI and release runners are pinned to that same version, so the floor is what gets tested. Raise both together.

## Release workflow

`.github/workflows/release.yml` is maintained by hand rather than generated, as uv's is, so it can pin actions by SHA and pass zizmor. `allow-dirty = ["ci"]` in `dist-workspace.toml` tells dist not to compare it with its own template; the `release plan` CI job runs `dist plan` instead. cargo-dist is installed like every other tool, from `.config/cargo-tools.toml` through the setup action, so its version is pinned in one place and fetched as a prebuilt release. The crate is `cargo-dist` but its binary is `dist`, not a cargo subcommand, so the workflows call `dist plan`, `dist build` and `dist host`. It needs these repository secrets:

| Secret | Holds |
| --- | --- |
| `APPLE_CERTIFICATE` | Base64-encoded Developer ID Application `.p12` |
| `APPLE_CERTIFICATE_PASSWORD` | Passphrase for the `.p12` |
| `APPLE_SIGNING_IDENTITY` | `Developer ID Application: Name (TEAMID)` |
| `APPLE_NOTARIZATION_USERNAME` | Apple ID email |
| `APPLE_NOTARIZATION_PASSWORD` | App-specific password for `notarytool` |
| `APPLE_TEAM_ID` | Ten-character team identifier |
| `HOMEBREW_TAP_TOKEN` | GitHub PAT with `repo` scope on the tap repository |

The Sigstore / build-provenance step, when added, needs `id-token: write` and `attestations: write` on the build job and nowhere else.

## Scheduled audit

A security advisory can be published against a dependency on a day when nobody pushes. `audit.yml` runs the dependency policy daily so that is still noticed.

## Renovate

Keeps every pinned version current, so the project never again sits months behind without anyone deciding that it should. Renovate replaces Dependabot because it can follow pins Dependabot cannot see:

- crates and `Cargo.lock` (lockfile only; manifests keep their ranges);
- GitHub Actions, pinned by SHA with the release in a comment;
- the toolchain in `rust-toolchain.toml`, grouped with `rust-version` in `Cargo.toml` so the two stay equal;
- the prek hook repositories in `.pre-commit-config.yaml`, through its built-in pre-commit manager;
- the cargo tools in `.config/cargo-tools.toml` and the matching `cargo-dist-version` in `dist-workspace.toml`.

The last two have no Renovate manager of their own, so each has a small custom one. Both files are data and nothing else, one `name = "version"` per line, which is what the patterns match.

Every update waits out the seven-day cooldown, refuses a release with no publication timestamp, and arrives as a Conventional Commit (`build(deps): …`, `ci(deps): …`) that passes the same hooks as any other change. Compatible crate updates are grouped into one pull request; majors arrive one by one because each needs reading. Security fixes skip the cooldown. A dependency dashboard issue lists everything pending.

Renovate runs as a GitHub App, which has to be installed on the repository once.

## Editor files

`.editorconfig` gives editors the settings the formatters enforce (UTF-8, LF, final newline, indent widths), so a file is right before a hook rewrites it. `.gitattributes` stores every text file with LF and marks audio fixtures binary, so Git never converts line endings inside a FLAC or MP3.
