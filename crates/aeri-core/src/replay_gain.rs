//! `ReplayGain` values and the factor they imply.

/// Gains beyond this many decibels either way are treated as a malformed tag.
const MAX_GAIN_DB: f32 = 60.0;

/// The `ReplayGain` values a file carries. Any of them may be missing.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ReplayGain {
    /// Gain that levels this track, in decibels.
    pub track_gain_db: Option<f32>,
    /// Largest sample magnitude in this track, where full scale is `1.0`.
    pub track_peak: Option<f32>,
    /// Gain that levels the album this track is on, in decibels.
    pub album_gain_db: Option<f32>,
    /// Largest sample magnitude in the album.
    pub album_peak: Option<f32>,
}

/// Which `ReplayGain` value to apply.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ReplayGainMode {
    /// Apply nothing.
    Off,
    /// Level each track alone, falling back to the album value.
    Track,
    /// Keep an album's loudness balance, falling back to the track value.
    #[default]
    Album,
}

impl ReplayGain {
    /// The linear factor to apply in `mode`, lowered so that `peak × factor ≤ 1`.
    ///
    /// `1.0` when the mode is off or the file carries no usable gain. A gain that is not
    /// finite, or beyond ±60 dB, is not usable; a peak that is not a positive, normal
    /// number is ignored. The peak used is the one from the same source as the gain.
    #[must_use]
    pub fn factor(&self, mode: ReplayGainMode) -> f32 {
        let track = (self.track_gain_db, self.track_peak);
        let album = (self.album_gain_db, self.album_peak);
        let (first, second) = match mode {
            ReplayGainMode::Off => return 1.0,
            ReplayGainMode::Track => (track, album),
            ReplayGainMode::Album => (album, track),
        };
        let Some((gain_db, peak)) = <[_; 2]>::from((first, second))
            .into_iter()
            .find_map(|(gain, peak)| usable_gain(gain).map(|gain| (gain, peak)))
        else {
            return 1.0;
        };

        let factor = 10.0_f32.powf(gain_db / 20.0);
        let Some(peak) = peak.filter(|peak| peak.is_normal() && peak.is_sign_positive()) else {
            return factor;
        };
        let capped = factor.min(peak.recip());
        if peak * capped > 1.0 {
            capped.next_down()
        } else {
            capped
        }
    }
}

fn usable_gain(gain: Option<f32>) -> Option<f32> {
    gain.filter(|db| db.is_finite() && db.abs() <= MAX_GAIN_DB)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    fn gain(track: Option<f32>, album: Option<f32>) -> ReplayGain {
        ReplayGain {
            track_gain_db: track,
            album_gain_db: album,
            ..ReplayGain::default()
        }
    }

    #[test]
    fn default_mode_is_album() {
        assert_eq!(ReplayGainMode::default(), ReplayGainMode::Album);
    }

    #[test]
    fn off_is_exactly_unity() {
        let all = ReplayGain {
            track_gain_db: Some(-6.0),
            track_peak: Some(0.5),
            album_gain_db: Some(-3.0),
            album_peak: Some(0.9),
        };
        assert_eq!(all.factor(ReplayGainMode::Off).to_bits(), 1.0_f32.to_bits());
    }

    #[test]
    fn no_values_is_unity_in_every_mode() {
        for mode in [ReplayGainMode::Track, ReplayGainMode::Album] {
            assert_eq!(
                ReplayGain::default().factor(mode).to_bits(),
                1.0_f32.to_bits()
            );
        }
    }

    #[test]
    fn each_mode_prefers_its_own_value() {
        let rg = gain(Some(-6.0), Some(-12.0));
        assert!(close(rg.factor(ReplayGainMode::Track), 0.501_187));
        assert!(close(rg.factor(ReplayGainMode::Album), 0.251_189));
    }

    #[test]
    fn a_missing_value_falls_back_to_the_other() {
        assert!(close(
            gain(Some(-6.0), None).factor(ReplayGainMode::Album),
            0.501_187
        ));
        assert!(close(
            gain(None, Some(-6.0)).factor(ReplayGainMode::Track),
            0.501_187
        ));
    }

    #[test]
    fn an_unusable_value_falls_back_like_a_missing_one() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 61.0, -61.0] {
            assert!(close(
                gain(Some(-6.0), Some(bad)).factor(ReplayGainMode::Album),
                0.501_187
            ));
        }
    }

    #[test]
    fn the_limit_itself_is_usable() {
        assert!(close(
            gain(Some(-60.0), None).factor(ReplayGainMode::Track),
            0.001
        ));
    }

    #[test]
    fn the_peak_lowers_a_boost_to_full_scale() {
        let rg = ReplayGain {
            track_gain_db: Some(6.0),
            track_peak: Some(0.8),
            ..ReplayGain::default()
        };
        assert!(close(rg.factor(ReplayGainMode::Track), 1.25));
    }

    #[test]
    fn a_peak_that_already_fits_changes_nothing() {
        let with = ReplayGain {
            track_gain_db: Some(6.0),
            track_peak: Some(0.4),
            ..ReplayGain::default()
        };
        let without = gain(Some(6.0), None);
        assert_eq!(
            with.factor(ReplayGainMode::Track).to_bits(),
            without.factor(ReplayGainMode::Track).to_bits()
        );
    }

    #[test]
    fn a_peak_of_exactly_full_scale_at_unity_gain_is_not_lowered() {
        let rg = ReplayGain {
            track_gain_db: Some(0.0),
            track_peak: Some(1.0),
            ..ReplayGain::default()
        };
        assert_eq!(
            rg.factor(ReplayGainMode::Track).to_bits(),
            1.0_f32.to_bits()
        );
    }

    #[test]
    fn a_boost_against_a_peak_of_full_scale_lands_exactly_on_unity() {
        let rg = ReplayGain {
            track_gain_db: Some(6.0),
            track_peak: Some(1.0),
            ..ReplayGain::default()
        };
        assert_eq!(
            rg.factor(ReplayGainMode::Track).to_bits(),
            1.0_f32.to_bits()
        );
    }

    #[test]
    fn an_attenuation_is_lowered_further_by_a_peak_above_full_scale() {
        let rg = ReplayGain {
            track_gain_db: Some(-1.0),
            track_peak: Some(2.0),
            ..ReplayGain::default()
        };
        assert_eq!(
            rg.factor(ReplayGainMode::Track).to_bits(),
            0.5_f32.to_bits()
        );
    }

    #[test]
    fn a_huge_peak_whose_reciprocal_rounds_up_is_still_kept_within_full_scale() {
        let peak = 8.510_657e37_f32;
        assert!(peak * peak.recip() > 1.0);
        let rg = ReplayGain {
            track_gain_db: Some(6.0),
            track_peak: Some(peak),
            ..ReplayGain::default()
        };
        assert!(peak * rg.factor(ReplayGainMode::Track) <= 1.0);
    }

    #[test]
    fn the_peak_comes_from_the_same_source_as_the_gain() {
        let rg = ReplayGain {
            track_gain_db: None,
            track_peak: Some(0.1),
            album_gain_db: Some(6.0),
            album_peak: Some(0.8),
        };
        assert!(close(rg.factor(ReplayGainMode::Track), 1.25));
    }

    #[test]
    fn a_useless_peak_is_ignored() {
        for peak in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            let rg = ReplayGain {
                track_gain_db: Some(6.0),
                track_peak: Some(peak),
                ..ReplayGain::default()
            };
            assert!(close(rg.factor(ReplayGainMode::Track), 1.995_262), "{peak}");
        }
    }

    fn any_value() -> impl Strategy<Value = Option<f32>> {
        prop::option::of(prop_oneof![
            -80.0_f32..80.0,
            any::<f32>(),
            Just(0.0),
            Just(f32::NAN),
            Just(f32::INFINITY),
        ])
    }

    fn any_replay_gain() -> impl Strategy<Value = ReplayGain> {
        (any_value(), any_value(), any_value(), any_value()).prop_map(
            |(track_gain_db, track_peak, album_gain_db, album_peak)| ReplayGain {
                track_gain_db,
                track_peak,
                album_gain_db,
                album_peak,
            },
        )
    }

    fn any_mode() -> impl Strategy<Value = ReplayGainMode> {
        prop_oneof![
            Just(ReplayGainMode::Off),
            Just(ReplayGainMode::Track),
            Just(ReplayGainMode::Album)
        ]
    }

    proptest! {
        #[test]
        fn the_factor_is_finite_and_positive(rg in any_replay_gain(), mode in any_mode()) {
            let factor = rg.factor(mode);
            prop_assert!(factor.is_finite() && factor > 0.0, "{factor}");
        }

        #[test]
        fn the_peak_never_clips(
            db in -60.0_f32..=60.0,
            peak in prop_oneof![
                0.001_f32..4.0,
                any::<f32>().prop_filter("positive and finite", |p| *p > 0.0 && p.is_finite()),
            ],
            track in any::<bool>(),
        ) {
            let (rg, mode) = if track {
                (ReplayGain { track_gain_db: Some(db), track_peak: Some(peak), ..ReplayGain::default() }, ReplayGainMode::Track)
            } else {
                (ReplayGain { album_gain_db: Some(db), album_peak: Some(peak), ..ReplayGain::default() }, ReplayGainMode::Album)
            };
            let factor = rg.factor(mode);
            prop_assert!(peak * factor <= 1.0, "{peak} × {factor}");
        }

        #[test]
        fn a_boost_with_a_peak_never_exceeds_full_scale(db in 0.0_f32..=60.0, peak in 0.001_f32..1.0) {
            let rg = ReplayGain { track_gain_db: Some(db), track_peak: Some(peak), ..ReplayGain::default() };
            prop_assert!(peak * rg.factor(ReplayGainMode::Track) <= 1.0);
        }
    }
}
