//! Conversions between frame counts and [`Duration`].

use std::time::Duration;

use aeri_core::SampleRate;

/// The time `frames` take at `rate`, rounded down to the nanosecond.
pub(crate) fn frames_to_duration(frames: u64, rate: SampleRate) -> Duration {
    let rate = u64::from(rate.get());
    // `rem < rate <= u32::MAX`, so `rem * 1e9` cannot overflow a u64.
    let rem = frames % rate;
    Duration::from_secs(frames / rate) + Duration::from_nanos(rem * 1_000_000_000 / rate)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn rate(hz: u32) -> SampleRate {
        SampleRate::new(hz).expect("test rates are non-zero")
    }

    #[test]
    fn one_second_of_frames_is_one_second() {
        assert_eq!(
            frames_to_duration(44_100, rate(44_100)),
            Duration::from_secs(1)
        );
    }

    #[test]
    fn a_partial_second_keeps_its_fraction() {
        assert_eq!(
            frames_to_duration(66_150, rate(44_100)),
            Duration::from_millis(1_500)
        );
    }

    proptest! {
        #[test]
        #[cfg_attr(miri, ignore = "proptest reads its regression file, which Miri's isolation forbids")]
        fn equals_the_exact_quotient_rounded_down(frames: u64, hz in 1..=u32::MAX) {
            let exact = u128::from(frames) * 1_000_000_000 / u128::from(hz);
            prop_assert_eq!(frames_to_duration(frames, rate(hz)).as_nanos(), exact);
        }
    }
}
