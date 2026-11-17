//! The built-in nodes, driven through the registry as an engine would.

#[cfg(test)]
mod tests {
    use aeri_alloc_guard::assert_no_alloc;
    use aeri_core::{Channels, ReplayGain, ReplayGainMode, SampleRate, StreamSpec, Volume};
    use aeri_dsp::{Chain, Control, DEFAULT_CHAIN, REPLAYGAIN, Registry, TrackStart, VOLUME};
    use proptest::prelude::*;

    fn channels(count: u16) -> Channels {
        Channels::new(count).expect("test channel counts are non-zero")
    }

    fn track(width: u16, replay_gain: ReplayGain) -> TrackStart {
        let spec = StreamSpec {
            sample_rate: SampleRate::new(44_100).expect("non-zero"),
            channels: channels(width),
        };
        TrackStart::new(spec, replay_gain)
    }

    fn chain(ids: &[&str], width: u16, replay_gain: ReplayGain) -> Chain {
        let mut chain = Registry::builtin().build(ids).expect("built-in ids");
        chain.start_track(&track(width, replay_gain));
        chain
    }

    fn volume(db: f32) -> Control {
        Control::Volume(Volume::from_db(db))
    }

    fn tagged(track_gain_db: f32) -> ReplayGain {
        ReplayGain {
            track_gain_db: Some(track_gain_db),
            ..ReplayGain::default()
        }
    }

    fn run(chain: &mut Chain, width: u16, block: &[f32]) -> Vec<f32> {
        let mut out = block.to_vec();
        chain.process(&mut out, channels(width));
        out
    }

    fn bits(samples: &[f32]) -> Vec<u32> {
        samples.iter().map(|sample| sample.to_bits()).collect()
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn the_default_chain_is_replaygain_then_volume() {
        assert_eq!(DEFAULT_CHAIN, [REPLAYGAIN, VOLUME]);
        assert!(Registry::builtin().build(DEFAULT_CHAIN).is_ok());
    }

    #[test]
    fn unity_volume_and_no_replaygain_leave_every_sample_untouched() {
        let mut chain = chain(DEFAULT_CHAIN, 2, ReplayGain::default());
        let block = [0.25, -0.5, f32::MIN_POSITIVE, -0.0, 1.0, f32::NAN];
        assert_eq!(bits(&run(&mut chain, 2, &block)), bits(&block));
    }

    #[test]
    fn replaygain_off_ignores_the_tags() {
        let mut chain = chain(DEFAULT_CHAIN, 1, tagged(-6.0));
        chain.control(&Control::ReplayGain(ReplayGainMode::Off));
        chain.start_track(&track(1, tagged(-6.0)));
        assert_eq!(run(&mut chain, 1, &[0.5, 0.5]), [0.5, 0.5]);
    }

    #[test]
    fn replaygain_scales_by_the_factor_for_the_track_and_mode() {
        let mut chain = chain(&[REPLAYGAIN], 1, tagged(-6.0));
        let out = run(&mut chain, 1, &[1.0, 1.0]);
        assert!(
            out.iter().all(|sample| close(*sample, 0.501_187)),
            "{out:?}"
        );
    }

    #[test]
    fn a_new_track_changes_the_gain_from_its_first_frame() {
        let mut chain = chain(&[REPLAYGAIN], 1, tagged(-6.0));
        let _ = run(&mut chain, 1, &[1.0; 8]);
        chain.start_track(&track(1, tagged(-12.0)));
        let out = run(&mut chain, 1, &[1.0; 8]);
        assert!(
            out.iter().all(|sample| close(*sample, 0.251_189)),
            "{out:?}"
        );
    }

    #[test]
    fn changing_the_replaygain_mode_ramps_rather_than_steps() {
        let both = ReplayGain {
            track_gain_db: Some(-6.0),
            album_gain_db: Some(-12.0),
            ..ReplayGain::default()
        };
        let mut chain = chain(&[REPLAYGAIN], 1, both);
        chain.control(&Control::ReplayGain(ReplayGainMode::Track));
        let _ = run(&mut chain, 1, &[1.0; 4]);
        chain.control(&Control::ReplayGain(ReplayGainMode::Album));
        let out = run(&mut chain, 1, &[1.0; 16]);
        assert!(out.windows(2).all(|pair| pair[1] <= pair[0]), "{out:?}");
        assert!(out[0] < 0.501_187 && out[0] > 0.251_189, "{out:?}");
        assert!(close(out[15], 0.251_189), "{out:?}");
    }

    #[test]
    fn half_volume_ramps_across_the_next_block_then_holds() {
        let mut chain = chain(&[VOLUME], 1, ReplayGain::default());
        chain.control(&Control::Volume(
            Volume::from_linear(0.5).expect("in range"),
        ));
        let ramp = run(&mut chain, 1, &[1.0; 100]);
        assert!(ramp.windows(2).all(|pair| pair[1] < pair[0]), "{ramp:?}");
        assert!(ramp[0] < 1.0 && ramp[0] > 0.5);
        assert_eq!(ramp[99].to_bits(), 0.5_f32.to_bits());
        assert_eq!(run(&mut chain, 1, &[1.0; 3]), [0.5; 3]);
    }

    #[test]
    fn a_ramp_applies_one_gain_to_every_channel_of_a_frame() {
        let mut chain = chain(&[VOLUME], 2, ReplayGain::default());
        chain.control(&volume(-20.0));
        let out = run(&mut chain, 2, &[1.0; 64]);
        assert!(
            out.as_chunks::<2>()
                .0
                .iter()
                .all(|frame| frame[0].to_bits() == frame[1].to_bits())
        );
        assert!(out[0] > out[62]);
    }

    #[test]
    fn a_new_target_during_a_ramp_starts_from_where_it_is() {
        let mut chain = chain(&[VOLUME], 1, ReplayGain::default());
        chain.control(&Control::Volume(Volume::SILENT));
        let first = run(&mut chain, 1, &[1.0; 10]);
        chain.control(&Control::Volume(Volume::UNITY));
        let second = run(&mut chain, 1, &[1.0; 10]);
        assert_eq!(first[9].to_bits(), 0.0_f32.to_bits());
        assert!(second[0] > 0.0 && second[0] < 0.2, "{second:?}");
        assert_eq!(second[9].to_bits(), 1.0_f32.to_bits());
    }

    #[test]
    fn a_reset_drops_a_ramp_in_progress() {
        let mut chain = chain(&[VOLUME], 1, ReplayGain::default());
        chain.control(&Control::Volume(Volume::SILENT));
        chain.reset();
        assert_eq!(run(&mut chain, 1, &[1.0; 4]), [0.0; 4]);
    }

    #[test]
    fn a_reset_keeps_the_volume() {
        let mut chain = chain(&[VOLUME], 1, ReplayGain::default());
        chain.control(&Control::Volume(
            Volume::from_linear(0.5).expect("in range"),
        ));
        let _ = run(&mut chain, 1, &[1.0; 4]);
        chain.reset();
        assert_eq!(run(&mut chain, 1, &[1.0; 2]), [0.5, 0.5]);
    }

    #[test]
    fn replaygain_survives_a_reset() {
        let mut chain = chain(&[REPLAYGAIN], 1, tagged(-6.0));
        chain.reset();
        assert!(close(run(&mut chain, 1, &[1.0])[0], 0.501_187));
    }

    #[test]
    fn each_node_ignores_the_other_nodes_controls() {
        let mut chain = chain(DEFAULT_CHAIN, 1, ReplayGain::default());
        chain.control(&Control::ReplayGain(ReplayGainMode::Track));
        assert_eq!(run(&mut chain, 1, &[0.5]), [0.5]);
        chain.control(&Control::Volume(Volume::UNITY));
        assert_eq!(run(&mut chain, 1, &[0.5]), [0.5]);
    }

    #[test]
    fn an_empty_block_and_a_partial_frame_are_left_alone_by_a_ramp() {
        let mut chain = chain(&[VOLUME], 2, ReplayGain::default());
        chain.control(&Control::Volume(Volume::SILENT));
        assert_eq!(run(&mut chain, 2, &[]), [] as [f32; 0]);
        let out = run(&mut chain, 2, &[1.0, 1.0, 1.0]);
        assert_eq!(out, [0.0, 0.0, 1.0]);
    }

    #[test]
    fn process_allocates_nothing_in_any_state() {
        let mut chain = chain(DEFAULT_CHAIN, 2, tagged(-6.0));
        let mut block = vec![0.5_f32; 2048];
        assert_no_alloc(|| chain.process(&mut block, channels(2)));
        chain.control(&volume(-10.0));
        assert_no_alloc(|| chain.process(&mut block, channels(2)));
        assert_no_alloc(|| chain.process(&mut block, channels(2)));
        chain.control(&Control::Volume(Volume::UNITY));
        chain.control(&Control::ReplayGain(ReplayGainMode::Off));
        assert_no_alloc(|| chain.process(&mut block, channels(2)));
        assert_no_alloc(|| chain.process(&mut block, channels(2)));
    }

    proptest! {
        #[test]
        #[cfg_attr(miri, ignore = "proptest reads its regression file, which Miri's isolation forbids")]
        fn volume_never_raises_a_sample(
            db in -90.0_f32..=0.0,
            width in 1_u16..=4,
            block in prop::collection::vec(-1.0_f32..=1.0, 0..3000),
        ) {
            let mut chain = chain(&[VOLUME], width, ReplayGain::default());
            chain.control(&volume(db));
            let out = run(&mut chain, width, &block);
            for (before, after) in block.iter().zip(&out) {
                prop_assert!(after.abs() <= before.abs());
            }
        }

        #[test]
        #[cfg_attr(miri, ignore = "proptest reads its regression file, which Miri's isolation forbids")]
        fn a_ramp_is_monotonic_and_ends_on_the_target(
            start_db in -90.0_f32..=0.0,
            end_db in -90.0_f32..=0.0,
            frames in 1_usize..2000,
        ) {
            let mut chain = chain(&[VOLUME], 1, ReplayGain::default());
            chain.control(&volume(start_db));
            let _ = run(&mut chain, 1, &[1.0]);
            chain.control(&volume(end_db));
            let out = run(&mut chain, 1, &vec![1.0; frames]);
            let rising = Volume::from_db(end_db) >= Volume::from_db(start_db);
            for pair in out.windows(2) {
                let ordered = if rising { pair[1] >= pair[0] } else { pair[1] <= pair[0] };
                prop_assert!(ordered);
            }
            prop_assert_eq!(out[frames - 1].to_bits(), Volume::from_db(end_db).linear().to_bits());
        }

        #[test]
        #[cfg_attr(miri, ignore = "proptest reads its regression file, which Miri's isolation forbids")]
        fn an_attenuating_replaygain_never_raises_a_sample(
            db in -60.0_f32..=0.0,
            block in prop::collection::vec(-1.0_f32..=1.0, 0..3000),
        ) {
            let mut chain = chain(&[REPLAYGAIN], 2, tagged(db));
            let out = run(&mut chain, 2, &block);
            for (before, after) in block.iter().zip(&out) {
                prop_assert!(after.abs() <= before.abs());
            }
        }
    }
}
