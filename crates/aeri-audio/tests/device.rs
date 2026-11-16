//! Plays through the real default output device. Run by hand and listen (brief, Step 4):
//! `cargo test -p aeri-audio --test device -- --ignored`

#[cfg(test)]
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod tests {
    use std::f32::consts::TAU;
    use std::thread;
    use std::time::{Duration, Instant};

    #[cfg(target_os = "macos")]
    use aeri_audio::CoreAudioSink as DeviceSink;
    use aeri_audio::OutputSink as _;
    #[cfg(target_os = "linux")]
    use aeri_audio::PipeWireSink as DeviceSink;
    use aeri_core::{Channels, SampleRate, StreamSpec};

    #[test]
    #[ignore = "plays a tone through the speakers; run by hand and listen for glitches"]
    fn plays_two_seconds_of_a_quiet_sine_without_gaps() {
        let rate = 44_100_u16;
        let spec = StreamSpec {
            sample_rate: SampleRate::new(rate.into()).expect("non-zero"),
            channels: Channels::new(2).expect("non-zero"),
        };
        let step = TAU * 440.0 / f32::from(rate);
        let samples: Vec<f32> = (0..u32::from(rate) * 2)
            .scan(0.0_f32, |phase, _| {
                let sample = phase.sin() * 0.2;
                *phase = (*phase + step) % TAU;
                Some(sample)
            })
            .flat_map(|sample| [sample, sample])
            .collect();

        let mut sink = DeviceSink::new();
        assert_eq!(sink.configure(spec).expect("default output opens"), spec);

        let mut rest = samples.as_slice();
        while !rest.is_empty() {
            let frames = sink.write(rest).expect("the device stays open");
            rest = rest.get(frames * 2..).unwrap_or_default();
            if frames == 0 {
                thread::sleep(Duration::from_millis(5));
            }
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while !sink.is_drained() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }

        assert!(sink.is_drained(), "the callback stopped consuming");
        assert_eq!(sink.frames_played(), u64::from(rate) * 2);
    }
}
