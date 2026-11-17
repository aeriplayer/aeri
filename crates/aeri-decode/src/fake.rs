//! An in-memory decoder and opener for tests of code that plays tracks (ADR 0029).
//!
//! [`fake_open`] mirrors the committed fixtures in `tests/fixtures`: the same file names, and for
//! each the spec, duration and `ReplayGain` the real decoder reports. A conformance test keeps
//! it that way. The audio is a sawtooth computed from the frame number with plain arithmetic,
//! so it is identical on every run and under Miri, which perturbs maths functions.

use std::ffi::OsStr;
use std::io;
use std::time::Duration;

use aeri_core::{Channels, ReplayGain, SampleRate, Source, StreamSpec};

use crate::{DecodeError, Decoder};

/// Frames in each block, near what the FLAC fixtures produce.
const BLOCK_FRAMES: u64 = 4_096;

/// Frames in one cycle of the sawtooth.
const PERIOD: u64 = 100;

/// The sawtooth's peak, like the fixtures' sine.
const PEAK: f32 = 0.088;

/// A decoder that produces a sawtooth of a given length, with no file behind it.
#[derive(Debug)]
pub struct FakeDecoder {
    spec: StreamSpec,
    frames: u64,
    replay_gain: ReplayGain,
    position: u64,
    block: Vec<f32>,
}

impl FakeDecoder {
    /// A stream of `frames` frames in `spec`, tagged with `replay_gain`.
    #[must_use]
    pub const fn new(spec: StreamSpec, frames: u64, replay_gain: ReplayGain) -> Self {
        Self {
            spec,
            frames,
            replay_gain,
            position: 0,
            block: Vec::new(),
        }
    }
}

/// The sample at `frame`, the same on every channel.
fn sawtooth(frame: u64) -> f32 {
    let phase = u16::try_from(frame % PERIOD).unwrap_or_default();
    (f32::from(phase) / 50.0 - 1.0) * PEAK
}

impl Decoder for FakeDecoder {
    fn spec(&self) -> StreamSpec {
        self.spec
    }

    fn duration(&self) -> Option<Duration> {
        Some(self.spec.sample_rate.duration_of(self.frames))
    }

    fn replay_gain(&self) -> ReplayGain {
        self.replay_gain
    }

    fn next_block(&mut self) -> Result<Option<&[f32]>, DecodeError> {
        let count = BLOCK_FRAMES.min(self.frames - self.position);
        if count == 0 {
            return Ok(None);
        }
        let width = usize::from(self.spec.channels.get());
        self.block.clear();
        for frame in self.position..self.position + count {
            let sample = sawtooth(frame);
            self.block.extend(std::iter::repeat_n(sample, width));
        }
        self.position += count;
        Ok(Some(&self.block))
    }

    fn seek(&mut self, to: Duration) -> Result<Duration, DecodeError> {
        let rate = u128::from(self.spec.sample_rate.get());
        let frame = u64::try_from(to.as_nanos() * rate / 1_000_000_000)
            .map_err(|_| DecodeError::Seek("position is out of range"))?;
        if frame > self.frames {
            return Err(DecodeError::Seek("position is out of range"));
        }
        self.position = frame;
        Ok(self.spec.sample_rate.duration_of(frame))
    }
}

fn cd_quality(hz: u32) -> StreamSpec {
    StreamSpec {
        sample_rate: SampleRate::new(hz).expect("a fixture rate is not zero"),
        channels: Channels::new(2).expect("two is not zero"),
    }
}

/// One second of a fixture's audio in the spec the fixture has.
fn one_second(hz: u32, replay_gain: ReplayGain) -> Box<dyn Decoder> {
    Box::new(FakeDecoder::new(cd_quality(hz), u64::from(hz), replay_gain))
}

/// Opens the fixture named by `source`'s file name, in memory.
///
/// # Errors
///
/// As [`open`](crate::open) would for the fixture directory: [`DecodeError::Io`] for a name that
/// is not a fixture, as for a missing file, and [`DecodeError::Unsupported`] for `not-audio.flac`.
pub fn fake_open(source: &Source) -> Result<Box<dyn Decoder>, DecodeError> {
    let Source::File(path) = source else {
        return Err(DecodeError::Unsupported("this kind of source"));
    };
    let tagged = ReplayGain {
        track_gain_db: Some(-6.54),
        track_peak: Some(0.088),
        album_gain_db: Some(-3.21),
        album_peak: Some(0.5),
    };
    match path.file_name().and_then(OsStr::to_str) {
        Some("sine.flac" | "sine.mp3") => Ok(one_second(44_100, ReplayGain::default())),
        Some("sine-48k.flac") => Ok(one_second(48_000, ReplayGain::default())),
        Some("sine-rg.flac" | "sine-rg.mp3") => Ok(one_second(44_100, tagged)),
        Some("sine-rg-bad.flac") => Ok(one_second(
            44_100,
            ReplayGain {
                track_gain_db: tagged.track_gain_db,
                ..ReplayGain::default()
            },
        )),
        Some("not-audio.flac") => Err(DecodeError::Unsupported("not a FLAC or MP3 stream")),
        _ => Err(DecodeError::Io(io::ErrorKind::NotFound.into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(name: &str) -> Source {
        Source::File(name.into())
    }

    fn drain(decoder: &mut dyn Decoder) -> (u64, Vec<f32>) {
        let width = u64::from(decoder.spec().channels.get());
        let (mut frames, mut all) = (0, Vec::new());
        while let Some(block) = decoder.next_block().expect("a fake does not fail") {
            assert_ne!(block.len(), 0, "a block is never empty");
            assert_eq!(block.len() as u64 % width, 0, "whole frames only");
            frames += block.len() as u64 / width;
            all.extend_from_slice(block);
        }
        (frames, all)
    }

    #[test]
    fn a_fixture_name_gives_one_second_in_its_own_spec() {
        for (name, hz) in [
            ("sine.flac", 44_100),
            ("sine.mp3", 44_100),
            ("sine-48k.flac", 48_000),
        ] {
            let mut decoder = fake_open(&source(name)).expect("a fixture opens");
            assert_eq!(decoder.spec(), cd_quality(hz), "{name}");
            assert_eq!(decoder.duration(), Some(Duration::from_secs(1)), "{name}");
            assert_eq!(drain(&mut *decoder).0, u64::from(hz), "{name}");
        }
    }

    #[test]
    fn the_directory_a_fixture_is_in_does_not_matter() {
        assert!(fake_open(&source("/anywhere/at/all/sine.flac")).is_ok());
    }

    #[test]
    fn the_tagged_fixtures_carry_replaygain_and_the_bad_one_keeps_one_value() {
        let tagged = fake_open(&source("sine-rg.flac"))
            .expect("opens")
            .replay_gain();
        assert_eq!(tagged.track_gain_db, Some(-6.54));
        assert_eq!(tagged.album_peak, Some(0.5));
        let bad = fake_open(&source("sine-rg-bad.flac"))
            .expect("opens")
            .replay_gain();
        assert_eq!(
            bad,
            ReplayGain {
                track_gain_db: Some(-6.54),
                ..ReplayGain::default()
            }
        );
        let plain = fake_open(&source("sine.flac"))
            .expect("opens")
            .replay_gain();
        assert_eq!(plain, ReplayGain::default());
    }

    #[test]
    fn a_name_that_is_not_a_fixture_is_a_missing_file() {
        let error = fake_open(&source("missing.flac")).err().expect("it fails");
        assert!(matches!(&error, DecodeError::Io(io) if io.kind() == io::ErrorKind::NotFound));
    }

    #[test]
    fn the_text_fixture_is_unsupported() {
        let error = fake_open(&source("not-audio.flac"))
            .err()
            .expect("it fails");
        assert!(matches!(error, DecodeError::Unsupported(_)));
    }

    #[test]
    fn the_audio_is_the_same_every_time_and_not_silent() {
        let (_, first) = drain(&mut *fake_open(&source("sine.flac")).expect("opens"));
        let (_, second) = drain(&mut *fake_open(&source("sine.flac")).expect("opens"));
        assert_eq!(first, second);
        assert!(first.iter().any(|sample| sample.abs() > 0.05));
        assert!(first.iter().all(|sample| sample.abs() <= PEAK));
    }

    #[test]
    fn seeking_lands_on_the_frame_and_decoding_resumes_from_it() {
        let mut decoder = fake_open(&source("sine.flac")).expect("opens");
        let (_, whole) = drain(&mut *decoder);

        let landed = decoder.seek(Duration::from_millis(500)).expect("in range");
        assert_eq!(landed, Duration::from_millis(500));
        let (frames, rest) = drain(&mut *decoder);
        assert_eq!(frames, 22_050);
        assert_eq!(rest, whole[44_100..], "the second half is the same samples");
    }

    #[test]
    fn seeking_to_the_end_is_allowed_and_past_it_is_an_error() {
        let mut decoder = fake_open(&source("sine.flac")).expect("opens");
        assert_eq!(
            decoder
                .seek(Duration::from_secs(1))
                .expect("the end is in range"),
            Duration::from_secs(1)
        );
        assert!(decoder.next_block().expect("no error").is_none());
        assert!(matches!(
            decoder.seek(Duration::from_millis(1_001)),
            Err(DecodeError::Seek(_))
        ));
    }

    #[test]
    fn the_sawtooth_stays_inside_its_peak_over_a_whole_period() {
        let samples: Vec<f32> = (0..PERIOD).map(sawtooth).collect();
        assert!(samples.iter().all(|s| s.abs() <= PEAK));
        assert!(samples.contains(&(-PEAK)));
    }
}
