# aeri — Rust style

How Rust is written in this project, and why. The aim is code that shows the language at its best: invariants carried by types, errors that say what failed, concurrency that cannot race by construction. Clippy and the workspace lints enforce what a machine can check; this document covers what needs judgment.

Each rule gives the reason, because a rule without a reason gets "fixed" by the next person who finds it inconvenient.

## 1. Make invalid states unrepresentable

Reach for the type system before reaching for a comment or a runtime check.

```rust
// No: nothing stops `position` being set while stopped, or a zero sample rate.
struct State { playing: bool, paused: bool, position: Option<Duration>, sample_rate: u32 }

// Yes: the shapes that cannot happen cannot be written.
enum PlaybackState {
    Stopped,
    Active { item: QueueItem, transport: Transport, position: Duration },
}
struct SampleRate(NonZeroU32);
```

- Two `bool`s that cannot both be true are an `enum`.
- An `Option` that is "always `Some` when X" belongs inside X's variant.
- A value with a rule (non-zero, in range, non-empty) is a newtype with a private field and one checked constructor. Validate once at the boundary; everything inside trusts the type. This is "parse, don't validate".
- A function that takes two values of the same primitive type (`fn seek(a: u64, b: u64)`) wants newtypes, so the arguments cannot be swapped.

## 2. Errors

- **Library crates** define their own error `enum` with `thiserror`, one per crate or per major operation, named for what failed (`DecodeError`, not `Error`). Mark it `#[non_exhaustive]`. Keep the underlying cause with `#[source]` or `#[from]`.
- **Binaries and `xtask`** use `anyhow::Result` and add context at each layer with `.context("what we were doing")`.
- Variants exist for failures a caller would handle *differently*. If no caller could tell two apart, they are one variant.
- **No `unwrap()`** outside tests (lint-enforced). `expect("…")` is allowed only when the message states why failure is impossible: `.expect("capacity was checked non-zero above")`. If you cannot write that sentence truthfully, return an error.
- **No panics for bad input.** A corrupt file, a missing device, and a full disk are ordinary results. Panics are for broken invariants in our own code.
- Error messages are lowercase, without trailing punctuation, and do not repeat their source: `"failed to open output device"`, not `"Error: Failed to open device: {e}."`.

## 3. Ownership and borrowing

- Take the weakest thing that works: `&str` over `&String`, `&[T]` over `&Vec<T>`, `&Path` over `&PathBuf`, `impl AsRef<Path>` at public entry points.
- Take ownership (`String`, `Vec<T>`) only when the function keeps the value.
- A `.clone()` added to satisfy the borrow checker is a question, not an answer: who should own this? Restructure before cloning.
- Shared ownership across threads is `Arc<T>`, written `Arc::clone(&x)` (lint-enforced) so a pointer copy is never mistaken for a deep copy.
- Return iterators (`impl Iterator<Item = T>`) rather than collecting into a `Vec` the caller may not need.

## 4. Traits and generics

- **Static dispatch by default** (`fn f<S: OutputSink>(s: S)` or `impl Trait`). Use `Box<dyn Trait>` when the concrete type is chosen at run time or when a generic parameter would spread through every caller for no measurable gain. Say which reason applies in a comment.
- **Async in traits is native.** Declare public trait methods as `fn name(&self) -> impl Future<Output = T> + Send`; implement them with `async fn`. Do not add the `async-trait` crate.
- Implement the standard traits instead of inventing methods: `From`/`TryFrom` for conversions, `Display` for user-facing text, `FromStr` for parsing, `Default` when there is one obvious default, `Iterator` for sequences.
- Every public type derives `Debug` (lint-enforced). Derive `Clone`, `PartialEq`, `Eq`, `Hash`, `Copy` when they are truthful, not by reflex — `Copy` on a type that may grow a heap field is a future breaking change.
- `#[non_exhaustive]` on public enums and structs that will gain variants or fields.
- `#[must_use]` on functions whose result is the whole point.

## 5. Concurrency

- **Share by communicating.** State owned by one thread and reached through channels (the actor pattern, ADR 0004) is preferred over `Arc<Mutex<T>>`. A mutex is acceptable for small, rarely contended data with no ordering concerns; say why in a comment.
- Never hold a lock across an `.await`.
- Blocking work (file I/O, decoding, SQLite) does not run on async worker threads: it gets its own thread or `spawn_blocking`.
- Long-lived threads are named (`std::thread::Builder::new().name(…)`) and joined on shutdown. Shutdown is driven by dropping the handle or closing the channel, not by a method callers must remember.
- Atomics: `Ordering::Relaxed` for independent counters and flags. If you need a stronger ordering to make two atomics agree, combine them into one value.

## 6. The real-time audio callback

The one place where ordinary good Rust is not enough. Inside the callback: no allocation or deallocation, no locks, no syscalls, no logging, no panics. Its only inputs are the ring buffer and atomics. The full rule list is in [`design/phase-2-3-playback.md`](design/phase-2-3-playback.md), step 4. Any change to callback code is reviewed against that list line by line.

## 7. `unsafe`

- Only in `aeri-audio`. Anywhere else, the design is wrong.
- Each `unsafe` block holds one operation and carries a `// SAFETY:` comment stating the invariant that makes it sound and who upholds it (lint-enforced).
- Wrap each FFI interaction in a small safe type whose constructor establishes the invariant and whose `Drop` releases the resource. The rest of the crate uses the safe type; `unsafe` does not leak into signatures.

## 8. Modules and visibility

- `pub` means part of the crate's public API. Everything else is private or `pub(crate)` (lint-enforced via `unreachable_pub`).
- `foo.rs` plus `foo/`, never `foo/mod.rs` (lint-enforced).
- A crate's `lib.rs` re-exports its public surface (`pub use`) so users write `aeri_core::Track`, not `aeri_core::track::Track`. No glob re-exports and no `prelude` modules.
- Third-party types stay out of public signatures unless they are the point (`std`, `tokio::sync` channel ends). symphonia, rusqlite, and platform FFI types are implementation details.

## 9. Documentation and tests

- Public items have rustdoc (lint-enforced). The first line is a one-sentence summary. Document `# Errors` for fallible functions and `# Panics` if any panic is reachable.
- Comments are the last resort, not the default; see [§12](#12-comments).
- Unit tests sit in `#[cfg(test)] mod tests` beside the code; behaviour that crosses modules is tested in `tests/`. Test names say what is expected: `removing_the_current_item_moves_current_to_its_successor`.
- Test through public APIs. A test that needs a private field is testing the wrong layer.
- Which technique each kind of code requires — property tests, fuzzing, Miri, `loom`, mutation testing — is set out in [`TESTING.md`](TESTING.md). It is not optional.

## 10. Suppressing a lint

Use `#[expect(lint, reason = "…")]` on the smallest item that needs it. `#[allow]` is rejected. `expect` turns into an error once the lint stops firing, so suppressions cannot outlive their cause. A crate-wide suppression needs an ADR.

## 11. Dependencies

Add one only when code needs it, with `cargo add`, then move the version into `[workspace.dependencies]`. Before adding: is it maintained, is its licence in `deny.toml`, does it duplicate something already in the tree, and could `std` do this? Prefer few features: `default-features = false` and enable what is used.

## 12. Comments

Code says *what*. Types, names and structure should make *how* readable. Anything longer than a sentence belongs somewhere it can be read, linked and kept current: a doc comment, a Markdown file, or an ADR. Inline comments are the exception.

**Write no comment unless deleting it would let a future reader make a mistake they cannot see coming.** The test is "what breaks if this is gone?" If the answer is "nothing, the code is still clear", delete it. Rename or extract instead.

A comment earns its place only as one of these, in one or two lines:

- a **hidden constraint**: an invariant, ordering, or limit that the code does not express and a tidy-up would break;
- a **workaround**, naming what it works around and, where known, when to remove it;
- a **surprising choice**, where the obvious alternative was tried or rejected;
- a **`// SAFETY:`** justification, or the `reason` of an `#[expect]` (lint-enforced);
- a **pointer** to the document that holds the longer reason (`// See ADR 0004`).

Never write:

- a comment that restates the line below it, or narrates steps ("// loop over tracks");
- history or provenance ("added for the queue work", "was X before"): that is `git log`;
- a TODO without an owner or an issue; none is better than an orphaned one;
- section banners or ASCII rules; split the file or the module instead;
- reasoning about *why this approach* in prose across several lines.

Where the explanation goes instead:

| What you want to say | Where it lives |
| --- | --- |
| What an item is, how to call it, what it returns or panics on | `///` rustdoc on the item |
| How a module or crate fits together | `//!` at the top of the module |
| Why a design was chosen over the alternative | An ADR in [`adr/`](adr/README.md) |
| Why a tool, hook, lint or CI job is configured as it is | [`TOOLING.md`](TOOLING.md) |
| Anything a newcomer must know before touching an area | A Markdown file in `docs/`, linked from the code |

The same rule governs configuration files (`.toml`, `.yml`). Do not describe what a key does; the tool's own documentation does that. Version markers written by tools (`# frozen: v1.1.12`, `# v7.0.1`) and `#:schema` directives are data and stay.

Doc comments are held to the same standard: a summary sentence, then only what the signature cannot say. Restating parameter names in prose is noise.
