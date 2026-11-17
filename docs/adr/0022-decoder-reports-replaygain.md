# ADR 0022 — The decoder reports ReplayGain, and lists the extensions it opens

- **Status:** Accepted
- **Date:** 2026-11-16

## Context

Phase 4 applies ReplayGain, and nothing reads tags until the library phase ([ADR 0009](0009-open-tag-metadata.md)). Two ways to get the values were on the table: add a tag reader (`lofty`) a phase early, which brings the tag-mapping question of ADR 0009 forward with it, or ask the decoder, which already parsed the tags when it probed the file.

Read in `symphonia` 0.6.1 on 2026-11-16:

- `FormatReader::metadata()` gives a log of revisions; each holds media-level tags and per-track tags, and a tag carries an optional `StandardTag`.
- Vorbis comments (FLAC), ID3v2 `TXXX` frames (MP3) and APE tags map their `replaygain_*` keys to `StandardTag::ReplayGain{Track,Album}{Gain,Peak}`, each holding the value as a string.
- `AudioDecoderOptions::gapless` defaults to `true`, and aeri-decode uses the default. The MP3 demuxer reads the LAME header's encoder delay and padding and the decoder trims them.
- The MP3 demuxer parses the LAME header's own ReplayGain fields but does not expose them.

The engine and the CLI also need to know which files can be opened, to filter a directory, without naming `aeri-decode`.

## Decision

- `Decoder` gains a required method, `fn replay_gain(&self) -> ReplayGain`. The values are read once, in `open`, from the metadata revision current when probing ends.
- A value is accepted as a decimal number, with an optional sign and an optional `dB` suffix in any case. One that does not parse, is not finite, or is negative where a peak is expected is dropped with a `tracing::warn!`. A bad tag never makes a file unplayable.
- `aeri-decode` exports `EXTENSIONS: &[&str]`, the lowercase extensions `open` can handle. The engine re-exports it.
- No dependency is added.

## Consequences

- `Decoder` is a public trait that crosses a crate boundary, so this change needs its ADR. The method is required, not defaulted, so an implementor cannot forget it. The only implementor is `SymphoniaDecoder`.
- Tags are read at `open`. A file retagged while it plays keeps the values it was opened with.
- Tags that follow the audio (an APE or ID3v1 footer) count only if the probe found them before returning. Not tested.
- The LAME header's ReplayGain is ignored. Files tagged only that way play at unity gain.
- `replaygain_reference_loudness` is ignored: gains are taken as written.
- Which tag formats really reach `StandardTag` for FLAC and MP3 is checked by fixture tests, not by the source reading above.
- Revisit when the library phase adds a tag reader. Two sources of tags would be one too many, and the decoder method would then go.
