---
name: aeri-review
description: Review aeri changes against the project's ADRs, Rust style guide, and real-time audio rules. Use before every commit that touches Rust code, when asked to review a diff or branch, and at the end of each design-brief step.
---

# aeri review

Review the change as a strict senior Rust reviewer who did not write it. The hooks
already check formatting, lints, tests, docs, and dependency policy; do not repeat
them. This review covers what a machine cannot check.

## Procedure

1. Get the change: `git diff --staged`, or `git diff main...HEAD` for a branch.
2. Read `docs/RUST_STYLE.md` and every ADR in `docs/adr/` that the touched crates are
   subject to. Read the current brief in `docs/design/`.
3. Run `prek run --all-files` and `prek run --all-files --stage pre-push`. If either
   fails, report that first and stop; do not review style on a red tree.
4. Walk the checklist below against the diff. Read the surrounding code, not only the
   changed lines.
5. Report findings, most severe first. For each: file and line, what is wrong, why it
   matters, and the concrete fix. Do not report preferences as defects.
6. End with a verdict: **ready**, **ready after fixes**, or **needs redesign**, and one
   sentence on what was not checked.

Do not fix anything unless asked. Never propose weakening a check as a fix.

## Checklist

### Fits the plan

- Does the change do what the current brief step asks, and nothing beyond it?
- Does it contradict an ADR, a locked decision, or a pillar in `ARCHITECTURE.md`? If
  so it needs a superseding ADR and the user's agreement, not a code review.
- Does it meet a trigger in `docs/adr/README.md` (new dependency, new crate, changed
  cross-crate trait, new thread or channel, new `unsafe`) without an ADR?
- New dependency: is it used, maintained, minimal in features, and was its API read
  from current docs rather than assumed?

### Types and API

- Can an invalid state be constructed? Look for paired `bool`s, `Option`s that are
  "always `Some` when…", raw `u32`/`f64`/`String` where a newtype carries a rule.
- Are two same-typed positional arguments swappable by mistake?
- Do public signatures expose third-party types that should be implementation details?
- Is anything `pub` that no other crate needs?
- Are enums and structs that will grow marked `#[non_exhaustive]`?
- Are derives truthful (`Copy`, `Eq`, `Default`), and is `Debug` useful?

### Errors

- Does each error variant correspond to something a caller would handle differently?
- Is the underlying cause preserved (`#[source]` / `#[from]` / `.context`)?
- Can bad *input* (file, device, network) cause a panic anywhere?
- Does every `expect("…")` message state why failure is impossible, and is it true?
- Are errors swallowed (`let _ =`, `.ok()`, empty `Err(_) =>`) without a stated reason?

### Ownership

- Any `.clone()` that exists only to quiet the borrow checker?
- Parameters stronger than needed (`&Vec<T>`, `&String`, `String` where `&str` works)?
- Allocation inside a loop that runs per audio block?

### Concurrency

- Is shared mutable state reached through a channel/actor, or is there a new
  `Arc<Mutex<…>>`? If a mutex, is it justified in a comment and never held across
  `.await`?
- Blocking work on an async worker thread?
- Every spawned thread named, and joined or otherwise accounted for on shutdown?
- Can a channel fill without bound? Can a dropped receiver cause a panic on send?

### Real-time audio callback (any change in or called from it)

Check each line. One violation is a defect regardless of how unlikely it looks.

- No allocation or deallocation, including hidden ones: `Vec` growth, `String`,
  `format!`, `Box`, a `Drop` that frees, cloning an `Arc` that may be the last.
- No locks, no channel operations, no `OnceLock`/`LazyLock` first-use.
- No syscalls, I/O, logging, or `tracing` macros.
- No panic paths: indexing, slicing, `unwrap`/`expect`, integer overflow, division.
- Inputs are only the ring buffer and atomics; atomics use `Relaxed` and no two must
  agree with each other.

### `unsafe`

- Is it in `aeri-audio`? Anywhere else is a redesign.
- Does each `// SAFETY:` comment name the actual invariant and who upholds it, and is
  that invariant really upheld on every path, including early returns and panics?
- Is the `unsafe` wrapped in a safe type so it does not appear in signatures?
- Are raw resources released in `Drop`?

### Tests

- Does new behaviour have a test that would fail without the change?
- Are tests asserting behaviour through public APIs, with names stating the
  expectation?
- Error paths and boundaries covered (empty, one, end of stream, device refused)?
- Does anything depend on a real audio device, wall-clock timing, or test order?

### Documentation

- Does rustdoc say what the item is *for* and document `# Errors` / `# Panics`?
- Would deleting each comment cost a reader something the code, names, and rustdoc
  cannot give (RUST_STYLE §12)? Flag narration, history, banners, multi-line
  reasoning that belongs in rustdoc or `docs/`.
- Are `CLAUDE.md` Status, the roadmap checklist, and affected docs updated?
