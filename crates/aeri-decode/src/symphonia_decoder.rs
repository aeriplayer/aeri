//! [`Decoder`] over symphonia.

use std::ffi::OsStr;
use std::path::Path;
use std::time::Duration;

use aeri_core::{Channels, SampleRate, StreamSpec};
use symphonia::core::audio::AudioSpec;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::{Error as SymphoniaError, SeekErrorKind};
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::{MediaSource, MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::{Time, TimeBase, Timestamp};

use crate::time::frames_to_duration;
use crate::{DecodeError, Decoder};

/// A FLAC or MP3 track, demuxed and decoded by symphonia.
pub(crate) struct SymphoniaDecoder {
    reader: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    spec: StreamSpec,
    duration: Option<Duration>,
    time_base: Option<TimeBase>,
    block: Vec<f32>,
}

impl SymphoniaDecoder {
    pub(crate) fn open_file(path: &Path) -> Result<Self, DecodeError> {
        let file = fs_err::File::open(path)?;
        let mut hint = Hint::new();
        if let Some(extension) = path.extension().and_then(OsStr::to_str) {
            hint.with_extension(extension);
        }
        Self::from_media_source(Box::new(file.into_file()), &hint)
    }

    #[cfg(fuzzing)]
    pub(crate) fn from_bytes(bytes: Vec<u8>) -> Result<Self, DecodeError> {
        Self::from_media_source(Box::new(std::io::Cursor::new(bytes)), &Hint::new())
    }

    fn from_media_source(source: Box<dyn MediaSource>, hint: &Hint) -> Result<Self, DecodeError> {
        let stream = MediaSourceStream::new(source, MediaSourceStreamOptions::default());
        let reader = symphonia::default::get_probe()
            .probe(
                hint,
                stream,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .map_err(convert)?;

        let track = reader
            .default_track(TrackType::Audio)
            .ok_or(DecodeError::Unsupported("no audio track"))?;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|params| params.audio())
            .ok_or(DecodeError::Unsupported("no audio track"))?;
        let sample_rate = params
            .sample_rate
            .and_then(SampleRate::new)
            .ok_or(DecodeError::Unsupported("stream states no sample rate"))?;
        let channels = params
            .channels
            .as_ref()
            .and_then(|channels| u16::try_from(channels.count()).ok())
            .and_then(Channels::new)
            .ok_or(DecodeError::Unsupported("stream states no channel count"))?;
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(params, &AudioDecoderOptions::default())
            .map_err(convert)?;

        let track_id = track.id;
        let time_base = track.time_base;
        let duration = track
            .num_frames
            .map(|frames| frames_to_duration(frames, sample_rate));

        Ok(Self {
            reader,
            decoder,
            track_id,
            spec: StreamSpec {
                sample_rate,
                channels,
            },
            duration,
            time_base,
            block: Vec::new(),
        })
    }

    fn timestamp_to_duration(&self, ts: Timestamp) -> Duration {
        self.time_base.map_or_else(
            || frames_to_duration(u64::try_from(ts.get()).unwrap_or(0), self.spec.sample_rate),
            |time_base| {
                time_base
                    .calc_time(ts)
                    .map_or(Duration::ZERO, time_to_duration)
            },
        )
    }
}

impl Decoder for SymphoniaDecoder {
    fn spec(&self) -> StreamSpec {
        self.spec
    }

    fn duration(&self) -> Option<Duration> {
        self.duration
    }

    fn next_block(&mut self) -> Result<Option<&[f32]>, DecodeError> {
        loop {
            let Some(packet) = self.reader.next_packet().map_err(convert)? else {
                return Ok(None);
            };
            if packet.track_id != self.track_id {
                continue;
            }
            match self.decoder.decode(&packet) {
                Ok(audio) => {
                    if !has_spec(audio.spec(), self.spec) {
                        tracing::warn!("skipping a packet whose format differs from the track's");
                        continue;
                    }
                    if audio.is_empty() {
                        continue;
                    }
                    audio.copy_to_vec_interleaved(&mut self.block);
                    return Ok(Some(&self.block));
                }
                Err(SymphoniaError::DecodeError(reason)) => {
                    tracing::warn!(reason, "skipping a corrupt packet");
                }
                Err(SymphoniaError::IoError(error)) => {
                    tracing::warn!(%error, "skipping an unreadable packet");
                }
                Err(error) => return Err(convert(error)),
            }
        }
    }

    fn seek(&mut self, to: Duration) -> Result<Duration, DecodeError> {
        let time = Time::try_from_nanos_u128(to.as_nanos())
            .ok_or(DecodeError::Seek("position is out of range"))?;
        let sought = self
            .reader
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time,
                    track_id: Some(self.track_id),
                },
            )
            .map_err(convert)?;
        self.decoder.reset();
        Ok(self.timestamp_to_duration(sought.actual_ts))
    }
}

fn has_spec(audio: &AudioSpec, spec: StreamSpec) -> bool {
    audio.rate() == spec.sample_rate.get()
        && audio.channels().count() == usize::from(spec.channels.get())
}

fn time_to_duration(time: Time) -> Duration {
    u128::try_from(time.as_nanos()).map_or(Duration::ZERO, Duration::from_nanos_u128)
}

fn convert(error: SymphoniaError) -> DecodeError {
    match error {
        SymphoniaError::IoError(error) => DecodeError::Io(error),
        SymphoniaError::DecodeError(reason) => DecodeError::Corrupt(reason),
        SymphoniaError::Unsupported(reason) | SymphoniaError::LimitError(reason) => {
            DecodeError::Unsupported(reason)
        }
        SymphoniaError::SeekError(kind) => DecodeError::Seek(seek_reason(&kind)),
        SymphoniaError::ResetRequired => {
            DecodeError::Unsupported("stream changes format mid-track")
        }
        _ => DecodeError::Unsupported("unrecognised decoder failure"),
    }
}

fn seek_reason(kind: &SeekErrorKind) -> &'static str {
    match kind {
        SeekErrorKind::Unseekable => "stream is not seekable",
        SeekErrorKind::ForwardOnly => "stream can only seek forward",
        SeekErrorKind::OutOfRange => "position is out of range",
        SeekErrorKind::InvalidTrack => "track is not in the stream",
        _ => "unrecognised seek failure",
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::*;

    #[test]
    fn io_errors_stay_io_errors() {
        let error = convert(SymphoniaError::IoError(io::Error::other("disk")));
        assert!(matches!(error, DecodeError::Io(inner) if inner.to_string() == "disk"));
    }

    #[test]
    fn malformed_data_is_corrupt() {
        let error = convert(SymphoniaError::DecodeError("bad frame"));
        assert!(matches!(error, DecodeError::Corrupt("bad frame")));
    }

    #[test]
    fn unsupported_features_and_limits_are_unsupported() {
        assert!(matches!(
            convert(SymphoniaError::Unsupported("codec")),
            DecodeError::Unsupported("codec")
        ));
        assert!(matches!(
            convert(SymphoniaError::LimitError("too large")),
            DecodeError::Unsupported("too large")
        ));
        assert!(matches!(
            convert(SymphoniaError::ResetRequired),
            DecodeError::Unsupported("stream changes format mid-track")
        ));
    }

    #[test]
    fn seek_errors_say_why() {
        let cases = [
            (SeekErrorKind::Unseekable, "stream is not seekable"),
            (SeekErrorKind::ForwardOnly, "stream can only seek forward"),
            (SeekErrorKind::OutOfRange, "position is out of range"),
            (SeekErrorKind::InvalidTrack, "track is not in the stream"),
        ];
        for (kind, reason) in cases {
            assert!(matches!(
                convert(SymphoniaError::SeekError(kind)),
                DecodeError::Seek(actual) if actual == reason
            ));
        }
    }

    #[test]
    fn a_block_has_the_spec_only_when_rate_and_channels_both_match() {
        use symphonia::core::audio::Channels as Layout;

        let spec = StreamSpec {
            sample_rate: SampleRate::new(44_100).expect("non-zero"),
            channels: Channels::new(2).expect("non-zero"),
        };

        assert!(has_spec(&AudioSpec::new(44_100, Layout::Discrete(2)), spec));
        assert!(!has_spec(
            &AudioSpec::new(48_000, Layout::Discrete(2)),
            spec
        ));
        assert!(!has_spec(
            &AudioSpec::new(44_100, Layout::Discrete(1)),
            spec
        ));
    }

    #[test]
    fn negative_times_clamp_to_zero() {
        assert_eq!(time_to_duration(Time::from_millis(-5)), Duration::ZERO);
        assert_eq!(
            time_to_duration(Time::from_millis(1_500)),
            Duration::from_millis(1_500)
        );
    }
}
