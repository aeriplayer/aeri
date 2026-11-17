//! Output gain.

/// Linear output gain, from silence to unity. Never above unity, never NaN.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Volume(f32);

impl Volume {
    /// No output.
    pub const SILENT: Self = Self(0.0);
    /// The signal unchanged.
    pub const UNITY: Self = Self(1.0);

    /// Returns the volume for the linear `gain`, or `None` if it is NaN or outside `0.0..=1.0`.
    #[must_use]
    pub fn from_linear(gain: f32) -> Option<Self> {
        (0.0..=1.0).contains(&gain).then_some(Self(gain))
    }

    /// Returns `db` decibels of gain. Values above 0 dB clamp to unity; NaN is silence.
    #[must_use]
    pub fn from_db(db: f32) -> Self {
        if db.is_nan() {
            Self::SILENT
        } else if db >= 0.0 {
            Self::UNITY
        } else {
            Self(10.0_f32.powf(db / 20.0))
        }
    }

    /// The gain as a multiplier.
    #[must_use]
    pub const fn linear(self) -> f32 {
        self.0
    }

    /// The gain in decibels: at most `0.0`, and negative infinity for silence.
    #[must_use]
    pub fn db(self) -> f32 {
        20.0 * self.0.log10()
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn from_linear_accepts_the_closed_unit_interval() {
        assert_eq!(Volume::from_linear(0.0), Some(Volume::SILENT));
        assert_eq!(Volume::from_linear(1.0), Some(Volume::UNITY));
        assert_eq!(Volume::from_linear(0.5).map(Volume::linear), Some(0.5));
    }

    #[test]
    fn from_linear_rejects_out_of_range_and_nan() {
        for gain in [
            -0.1,
            1.000_1,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            -1.0,
        ] {
            assert_eq!(Volume::from_linear(gain), None, "{gain}");
        }
    }

    #[test]
    fn from_db_clamps_above_zero_to_unity() {
        assert_eq!(Volume::from_db(0.0), Volume::UNITY);
        assert_eq!(Volume::from_db(6.0), Volume::UNITY);
        assert_eq!(Volume::from_db(f32::INFINITY), Volume::UNITY);
    }

    #[test]
    fn from_db_treats_nan_and_negative_infinity_as_silence() {
        assert_eq!(Volume::from_db(f32::NAN), Volume::SILENT);
        assert_eq!(Volume::from_db(f32::NEG_INFINITY), Volume::SILENT);
    }

    #[test]
    fn minus_six_db_is_about_half() {
        assert!((Volume::from_db(-6.0).linear() - 0.501_187).abs() < 1e-5);
    }

    #[test]
    fn db_of_the_extremes() {
        assert!(Volume::UNITY.db().abs() < f32::EPSILON);
        assert_eq!(Volume::SILENT.db(), f32::NEG_INFINITY);
    }

    proptest! {
        #[test]
        fn from_db_is_always_a_valid_volume(db in any::<f32>()) {
            let linear = Volume::from_db(db).linear();
            prop_assert!((0.0..=1.0).contains(&linear));
        }

        #[test]
        fn db_round_trips_through_from_db(db in -90.0_f32..0.0) {
            let back = Volume::from_db(db).db();
            prop_assert!((back - db).abs() < 1e-3, "{db} came back as {back}");
        }

        #[test]
        fn from_db_is_monotonic(a in -120.0_f32..0.0, b in -120.0_f32..0.0) {
            prop_assert_eq!(a <= b, Volume::from_db(a) <= Volume::from_db(b));
        }
    }
}
