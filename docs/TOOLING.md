# Tooling rationale

Why the repository's configuration files are shaped the way they are. The files themselves carry no prose beyond a line where a reader would otherwise "fix" something correct (see [`RUST_STYLE.md` §12](RUST_STYLE.md#12-comments)). Commands are in [`README.md`](../README.md); which verification each kind of code needs is in
[`TESTING.md`](TESTING.md).

## One definition of "passes checks"

Every check is defined once, in `prek.toml`. CI runs the same hooks rather than restating their commands, so a green hook and a green CI cannot disagree. CI only decides *where* each group runs:

- **Commit-msg stage:** the message itself (Conventional Commits, forbidden trailers).
- **Pre-commit stage:** fast, on staged files: hygiene, formatting, lints, clippy.
- **Pre-push stage:** slow, whole workspace: tests, docs, feature powerset, dependency policy.
- **Manual stage:** coverage, mutation testing, Miri. Too slow for every push; CI runs them on every pull request.

Hooks that only make sense on a developer machine (signed commits, no commits to `main`) are skipped in CI: CI builds `main` itself, and merge commits created by GitHub are not signed with a contributor key.

### Priorities

Hooks run in ascending priority; hooks sharing a priority run concurrently. Anything that rewrites files has a priority of its own, because two fixers touching one file at once is undefined. Cargo hooks are serialised as well: they would only queue on the build-directory lock.

### The `target` group

`language = "system"` runs the cargo that `rust-toolchain.toml` selects, so hooks, CI and `cargo build` use one compiler. Hooks in the `target` group (clippy, tests, docs, feature powerset) give different answers per platform because the audio backends are `cfg(target_os)`. CI runs that group on every supported platform and everything else once.

### Pinning and cooldown

`prek update` pins hooks to commit SHAs, because a tag can be moved and a SHA cannot, and holds back anything published in the last seven days so a compromised release has time to be noticed. Dependabot applies the same seven-day cooldown to crates and actions. The `# frozen: vX` and `# vX` markers beside SHAs are written by the tools; leave them.

### Deliberate gaps

- `check-hooks-apply` (prek's meta hook) is omitted: several hooks guard things the repository does not contain yet, such as symlinks.
- `LICENSE-*` files are excluded from every hook so they stay byte-for-byte as published.

## Hooks that need explaining

| Hook | Why it exists in this form |
| --- | --- |
| `cargo-nextest` | One process per test, hung tests killed, no retries (`.config/nextest.toml`). `--no-tests=pass` only matters while crates are empty; remove it with the first test. |
| `cargo-doctest` | nextest does not run documentation examples. |
| `cargo-hack` | Every feature combination must compile alone, so a gated boundary such as `aeri-audio`'s `test-util` feature cannot rot unnoticed. |
| `coverage` | Runs the tests once, then reports per floor: the workspace, and each pure-logic crate at 100% (`--package`). A crate with no code fails a 100% report, so each crate's line is added with its first code. Excludes `xtask/` and `build.rs`: build automation, not product code. The floor is a tripwire, not a goal ([`TESTING.md`](TESTING.md)). |
| `mutants` | Mutates only the lines that differ from the base branch, then requires the tests to notice every mutation. Excluded mutants are named in `.cargo/mutants.toml` with the reason. |
| `miri` | Needs `rustup toolchain install nightly --component miri`. Tests that cannot run under Miri (FFI, real I/O) opt out with `#[cfg_attr(miri, ignore)]`. |
| `cargo-deny` | Runs at push because it reads the advisory database over the network. |

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

`pedantic` and `nursery` sit at priority −1 so the individual levels win. `missing_const_for_fn` is allowed: it fires on functions that are incidentally const-able today but would become breaking changes to un-const later.

`clippy.toml` sets `too-many-arguments-threshold = 5`: beyond five positional arguments, use a struct.

## Toolchain

`rust-toolchain.toml` pins an exact stable so CI cannot break on an unrelated release. Bump it in its own commit with every check green. `llvm-tools` is there for `cargo-llvm-cov`.

The minimum macOS version is `MACOSX_DEPLOYMENT_TARGET` in `.cargo/config.toml`, which cargo passes to every compile and link. The macOS CI and release runners are pinned to that same version, so the floor is what gets tested. Raise both together.

## Release workflow

`.github/workflows/release.yml` is a generated skeleton for cargo-dist 0.32.0; run `cargo dist generate` for the canonical output. It needs these repository secrets:

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

## Dependabot

Keeps dependencies and CI actions current, so the project never again sits months behind without anyone deciding that it should. Every update arrives as a pull request and passes the same hooks as any other change. Compatible updates are grouped into one pull request; majors arrive one by one because each needs reading.
