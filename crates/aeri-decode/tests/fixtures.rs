//! Decodes the committed fixtures through the public API.

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use aeri_core::{Channels, SampleRate, Source, StreamSpec};
    use aeri_decode::{DecodeError, Decoder};

    const FRAMES: u64 = 44_100;
    /// One MPEG-1 Layer III frame: the most an MP3 decoder may be off by without gapless info.
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
    fn mp3_decodes_within_one_frame_of_the_source_length() {
        let mut decoder = open("sine.mp3");

        assert_eq!(decoder.spec(), stereo_44k());
        let (frames, peak) = drain(decoder.as_mut());
        assert!(
            frames.abs_diff(FRAMES) <= MP3_FRAME,
            "decoded {frames} frames"
        );
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
}
