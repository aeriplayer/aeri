# ADR 0010 — One script language, in its own crate, formats and queries tracks

- **Status:** Accepted
- **Date:** 2026-11-15

## Context

What makes foobar2000 and fooyin configurable is a small language used everywhere:
playlist columns (`%artist% - %title%`), grouping and sorting keys, the status bar,
file renaming, and, in query form (`genre HAS jazz AND rating GREATER 3`), search,
library filters, and autoplaylists. fooyin calls its version FooScript.

Without it, each of those features needs its own settings and its own code. With it,
each is a text field.

## Decision

`aeri-script` is a crate that depends only on `aeri-core` and provides:

- **title formatting:** an expression over a track that yields a string;
- **queries:** a predicate over a track that yields a boolean.

Both share one lexer, one field model (ADR 0009), and one function library. A script
is parsed once into a tree and evaluated many times.

The syntax follows foobar2000 title formatting and query syntax, so that what users
already know transfers. Where foobar2000 and fooyin differ, or where a construct is
costly, aeri chooses and documents the difference; byte-for-byte compatibility is not
a goal.

The crate does no I/O, holds no global state, and never panics on any input.

## Consequences

- The language is the largest piece of pure logic in aeri and the best fit for
  property tests and fuzzing. Both are required before it is used by another crate.
- Evaluation speed bounds library queries (ADR 0008): every track, per keystroke.
  Benchmark it from the first evaluator.
- Scripts are stored in settings and layouts, so the language is a compatibility
  surface from the first release.
- Formatting directives that affect appearance (colour, font) are GUI concerns layered
  on top, not part of this crate.
- Autoplaylists and library filters cost almost nothing once queries exist, which is
  why scripting precedes the GUI in the roadmap.
