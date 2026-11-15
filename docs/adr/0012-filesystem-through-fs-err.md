# ADR 0012 — Filesystem access goes through fs-err

- **Status:** Accepted
- **Date:** 2026-11-15

## Context

`std::fs` returns `io::Error`s that do not say which path failed: a library scan over thousands of files reports "No such file or directory (os error 2)" and nothing else. Adding the path by hand at every call site with `.with_context(…)` works until one call site forgets, and nothing catches the omission.

`fs-err` mirrors the `std::fs` API one-to-one and puts the path and the operation in every error. uv makes the same choice and enforces it with clippy's `disallowed-methods` and `disallowed-types`. The alternative on the table was a project-local wrapper module, which would duplicate `fs-err` without its testing or its users.

## Decision

- Code reads and writes files through `fs_err`, never `std::fs`. `clippy.toml` bans the `std::fs` free functions and the `File`, `OpenOptions`, `ReadDir` and `DirEntry` types, naming this ADR in the message.
- `fs-err` is declared once in `[workspace.dependencies]` with `default-features = false`. A crate adds it with its first filesystem call, as with every dependency. `xtask` is the first.
- Async filesystem code, if any arrives, uses `fs-err`'s `tokio` feature rather than `tokio::fs`. That ban is added with the first `tokio` dependency.

## Consequences

- Every I/O error names its path without effort at the call site.
- One more dependency in every crate that touches files. `fs-err` has no runtime dependencies and one build script, through `autocfg`. On 2026-11-15, 3.3.2 had no default features and nothing optional beyond `tokio`, `debug` and `expose_original_error`. That was read from the crates.io index, not from a full source review.
- Third-party crates still call `std::fs` internally (`rusqlite` opens its database file itself), so their errors are wrapped with context where we call them.
- `std::os::unix::fs` and platform extensions are not banned. Revisit if a crate starts using them.
- Revisit if `std::io::Error` gains path information of its own.
