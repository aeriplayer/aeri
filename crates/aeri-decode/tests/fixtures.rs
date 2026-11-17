//! Decodes the committed fixtures through the public API.

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use aeri_core::{Channels, ReplayGain, SampleRate, Source, StreamSpec};
    use aeri_decode::{DecodeError, Decoder};

    const FRAMES: u64 = 44_100;
    /// One MPEG-1 Layer III frame: how far a seek may land from its target.
    const MP3_FRAME: u64 = 1_152;
    /// The fixtures peak near 0.088 (see their README); silence and denormals sit far below.
    const AUDIBLE: f32 = 0.05;

    fn fixture(name: &str) -> Source {
        Source::File(
            [env!("CARGO_MANIFEST_DIR"), "tests", "fixtures", name]
                .iter()
                .collect::<PathBuf>(),
        )
    }

    fn open(name: &str) -> Box<dyn Decoder> {
        aeri_decode::open(&fixture(name)).expect("fixtures decode")
    }

    /// Decodes to the end; returns the frame count and the largest absolute sample.
    fn drain(decoder: &mut dyn Decoder) -> (u64, f32) {
        let channels = usize::from(decoder.spec().channels.get());
        let mut frames = 0;
        let mut peak = 0.0_f32;
        while let Some(block) = decoder.next_block().expect("fixtures decode cleanly") {
            assert!(!block.is_empty(), "blocks are never empty");
            assert_eq!(block.len() % channels, 0, "blocks hold whole frames");
            frames += (block.len() / channels) as u64;
            peak = block
                .iter()
                .fold(peak, |peak, sample| peak.max(sample.abs()));
        }
        (frames, peak)
    }

    fn stereo_44k() -> StreamSpec {
        StreamSpec {
            sample_rate: SampleRate::new(44_100).expect("non-zero"),
            channels: Channels::new(2).expect("non-zero"),
        }
    }

    #[test]
    #[cfg_attr(miri, ignore = "opens files, which Miri's isolation forbids")]
    fn flac_decodes_every_frame_of_a_loud_sine() {
        let mut decoder = open("sine.flac");

        assert_eq!(decoder.spec(), stereo_44k());
        assert_eq!(decoder.duration(), Some(Duration::from_secs(1)));
        let (frames, peak) = drain(decoder.as_mut());
        assert_eq!(frames, FRAMES);
        assert!(peak > AUDIBLE, "peak {peak} is near silence");
    }

    #[test]
    #[cfg_attr(miri, ignore = "opens files, which Miri's isolation forbids")]
    fn mp3_decodes_exactly_the_source_length_because_encoder_delay_is_trimmed() {
        let mut decoder = open("sine.mp3");

        assert_eq!(decoder.spec(), stereo_44k());
        assert_eq!(decoder.duration(), Some(Duration::from_secs(1)));
        let (frames, peak) = drain(decoder.as_mut());
        assert_eq!(frames, FRAMES);
        assert!(peak > AUDIBLE, "peak {peak} is near silence");
    }

    #[test]
    #[cfg_attr(miri, ignore = "opens files, which Miri's isolation forbids")]
    fn seeking_lands_at_or_before_the_target_and_decodes_the_rest() {
        let target = Duration::from_millis(500);
        for name in ["sine.flac", "sine.mp3"] {
            let mut decoder = open(name);

            let landed = decoder.seek(target).expect("fixtures are seekable");
            assert!(landed <= target, "{name}: landed at {landed:?}");
            assert!(
                landed >= target.saturating_sub(Duration::from_millis(100)),
                "{name}: landed at {landed:?}, far before the target"
            );

            let (frames, _) = drain(decoder.as_mut());
            let skipped = u64::try_from(landed.as_nanos() * u128::from(FRAMES) / 1_000_000_000)
                .expect("landed within the one-second fixture");
            assert!(
                frames.abs_diff(FRAMES - skipped) <= MP3_FRAME,
                "{name}: decoded {frames} frames after landing at {landed:?}"
            );
        }
    }

    #[test]
    #[cfg_attr(miri, ignore = "opens files, which Miri's isolation forbids")]
    fn a_missing_file_is_an_io_error() {
        let error = aeri_decode::open(&fixture("missing.flac")).err();

        assert!(matches!(error, Some(DecodeError::Io(_))), "{error:?}");
    }

    #[test]
    #[cfg_attr(miri, ignore = "opens files, which Miri's isolation forbids")]
    fn a_text_file_named_flac_is_unsupported() {
        let error = aeri_decode::open(&fixture("not-audio.flac")).err();

        assert!(
            matches!(error, Some(DecodeError::Unsupported(_))),
            "{error:?}"
        );
    }

    fn tagged() -> ReplayGain {
        ReplayGain {
            track_gain_db: Some(-6.54),
            track_peak: Some(0.088),
            album_gain_db: Some(-3.21),
            album_peak: Some(0.5),
        }
    }

    #[test]
    #[cfg_attr(miri, ignore = "opens files, which Miri's isolation forbids")]
    fn flac_reports_the_replaygain_in_its_vorbis_comments() {
        assert_eq!(open("sine-rg.flac").replay_gain(), tagged());
    }

    #[test]
    #[cfg_attr(miri, ignore = "opens files, which Miri's isolation forbids")]
    fn mp3_reports_the_replaygain_in_its_id3v2_tag() {
        assert_eq!(open("sine-rg.mp3").replay_gain(), tagged());
    }

    #[test]
    #[cfg_attr(miri, ignore = "opens files, which Miri's isolation forbids")]
    fn files_without_the_tags_report_none() {
        for name in ["sine.flac", "sine.mp3", "sine-48k.flac"] {
            assert_eq!(open(name).replay_gain(), ReplayGain::default(), "{name}");
        }
    }

    #[test]
    #[cfg_attr(miri, ignore = "opens files, which Miri's isolation forbids")]
    fn a_file_with_malformed_values_still_plays_and_keeps_the_good_one() {
        let mut decoder = open("sine-rg-bad.flac");

        assert_eq!(
            decoder.replay_gain(),
            ReplayGain {
                track_gain_db: Some(-6.54),
                ..ReplayGain::default()
            }
        );
        let (frames, _) = drain(decoder.as_mut());
        assert_eq!(frames, FRAMES);
    }

    #[test]
    #[cfg_attr(miri, ignore = "opens files, which Miri's isolation forbids")]
    fn tags_do_not_change_what_decodes() {
        for (plain, tagged) in [("sine.flac", "sine-rg.flac"), ("sine.mp3", "sine-rg.mp3")] {
            let (plain_frames, plain_peak) = drain(open(plain).as_mut());
            let (tagged_frames, tagged_peak) = drain(open(tagged).as_mut());
            assert_eq!(plain_frames, tagged_frames, "{tagged}");
            assert_eq!(plain_peak.to_bits(), tagged_peak.to_bits(), "{tagged}");
        }
    }

    #[test]
    #[cfg_attr(miri, ignore = "checks files exist, which Miri's isolation forbids")]
    fn every_extension_is_lowercase_and_has_a_fixture() {
        for extension in aeri_decode::EXTENSIONS {
            assert_eq!(*extension, extension.to_lowercase());
            assert!(!extension.starts_with('.'));
            let name = format!("sine.{extension}");
            assert!(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures")
                    .join(name)
                    .exists(),
                "no fixture for {extension}"
            );
        }
    }
}
