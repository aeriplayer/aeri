# ADR 0029 — The engine opens tracks through an opener, so its tests need no audio files

- **Status:** Accepted
- **Date:** 2026-11-17

## Context

`aeri-engine` calls `aeri_decode::open` in two places. Every test of the engine, the player, the actor and the CLI's controls therefore decodes real FLAC from the committed fixtures: about 90 tests. Those tests are about what the engine and player do with a stream (what plays next, when the slot is read, what happens on a failure), not about codecs.

Under Miri that is the cost: decoding one second of FLAC takes 77 seconds (measured 2026-11-17), and the decoder's 11 fixture tests had not finished after nine minutes. [AGENTS.md rule 3](../../AGENTS.md) makes each ignore an exception that needs a reason, and "it is slow" is not one for 88 tests. Miri also has little to say about them: all of aeri's `unsafe` is in `aeri-audio`, and symphonia is covered by fuzzing ([ADR 0015](0015-fuzzing-with-cargo-fuzz.md)). A test that needs a file to test a queue is pointing at a missing seam.

Alternatives considered:

- **Ignore the tests under Miri.** What we had. 88 attributes standing in for a missing seam.
- **Shorter fixtures.** Decoding still dominates, and tests that assert on the fixtures' exact lengths would change with them.
- **A generic `Engine<D>`.** Spreads a type parameter through `Player` and every frontend for a test-only need.

## Decision

- `Playback` holds an **opener**: `Box<dyn Fn(&Source) -> Result<Box<dyn Decoder>, DecodeError> + Send>`. `Engine::spawn` passes `aeri_decode::open`, so shipped behaviour is unchanged.
- Behind the `test-util` feature, `Engine::spawn_with(sink, events, opener)` takes any opener, and `Player::with_fake_sink` passes the fake one below.
- `aeri-decode` gains a `test-util` feature with `FakeDecoder` and `fake_open`. `fake_open` mirrors the committed fixtures: the same file names, the same spec, duration and ReplayGain, produced in memory as a deterministic waveform that needs no maths library. A name that is not a fixture is a missing file, and `not-audio.flac` is unsupported, as in the real decoder.
- A conformance test in `aeri-decode` opens each real fixture and its fake and compares spec, duration and ReplayGain, so the fake cannot drift from the files.
- Miri runs on every crate except `xtask`, `aeri-engine`, `aeri-player` and `aeri-cli`. Miri finds two kinds of fault: undefined behaviour in `unsafe` code, and floating-point results that tests or code assume to be exact. None of those four contains `unsafe` (the workspace lint forbids it) or floating-point arithmetic of its own: the engine, player and CLI do integer bookkeeping and call `aeri-dsp` for the arithmetic. Their tests check order and state, which native runs check as well. The seam makes them cheap enough to run under Miri if that ever changes; cost is not the reason. A crate that gains `unsafe` or floating-point arithmetic leaves the list.
- Tests whose subject is the codec or the path from file to engine keep using real files: the decoder's own tests, and two engine tests, one FLAC and one MP3. The five decoder tests that decode a whole file are the only tests ignored under Miri for cost, and the reason names the decoder. Opening a file, and reading its tags, is cheap and runs under Miri.

## Consequences

- The engine, player, actor and controls tests run under nextest in milliseconds, with no filesystem. They are outside Miri's scope, as above: with the seam they would cost about a minute each there, mostly in interpreting the playing of a one-second track.
- The fake is a second implementation of a stream. It is only as good as the conformance test: it checks the metadata, not that a seek lands where the real decoder lands. Tests of seek tolerance therefore test the engine's handling of a landing point, and the decoder's seek is tested by the decoder.
- Anything in the engine that depends on the real decoder's behaviour (block size, its error kinds) is exercised by the real-file tests only.
- `aeri-decode` and `aeri-engine` each gain a `test-util` feature, for dev-dependencies only, like `aeri-audio`'s.
- Revisit if a third decoder (Ogg, Opus) makes the real-file tests multiply.
