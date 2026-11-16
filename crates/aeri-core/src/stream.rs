//! The shape of a PCM stream.

use std::error::Error;
use std::fmt;
use std::num::{NonZeroU16, NonZeroU32};
use std::time::Duration;

/// Sample rate in Hz. Never zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SampleRate(NonZeroU32);

impl SampleRate {
    /// Returns the rate for `hz` samples per second, or `None` if `hz` is zero.
    #[must_use]
    pub const fn new(hz: u32) -> Option<Self> {
        match NonZeroU32::new(hz) {
            Some(hz) => Some(Self(hz)),
            None => None,
        }
    }

    /// Samples per second.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0.get()
    }

    /// How long `frames` frames last at this rate, rounded down to the nanosecond.
    #[must_use]
    pub fn duration_of(self, frames: u64) -> Duration {
        let rate = u64::from(self.get());
        // `rem < rate <= u32::MAX`, so `rem * 1e9` cannot overflow a u64.
        let rem = frames % rate;
        Duration::from_secs(frames / rate) + Duration::from_nanos(rem * 1_000_000_000 / rate)
    }
}

impl TryFrom<u32> for SampleRate {
    type Error = ZeroSampleRateError;

    fn try_from(hz: u32) -> Result<Self, Self::Error> {
        Self::new(hz).ok_or(ZeroSampleRateError)
    }
}

/// The error from converting `0` into a [`SampleRate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct ZeroSampleRateError;

impl fmt::Display for ZeroSampleRateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("sample rate must be non-zero")
    }
}

impl Error for ZeroSampleRateError {}

/// Number of interleaved channels. Never zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Channels(NonZeroU16);

impl Channels {
    /// Returns the channel count `count`, or `None` if `count` is zero.
    #[must_use]
    pub const fn new(count: u16) -> Option<Self> {
        match NonZeroU16::new(count) {
            Some(count) => Some(Self(count)),
            None => None,
        }
    }

    /// The number of channels.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0.get()
    }
}

impl TryFrom<u16> for Channels {
    type Error = ZeroChannelsError;

    fn try_from(count: u16) -> Result<Self, Self::Error> {
        Self::new(count).ok_or(ZeroChannelsError)
    }
}

/// The error from converting `0` into a [`Channels`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct ZeroChannelsError;

impl fmt::Display for ZeroChannelsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("channel count must be non-zero")
    }
}

impl Error for ZeroChannelsError {}

/// The shape of a PCM stream. Two streams are gapless-compatible iff their specs are equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StreamSpec {
    /// Frames per second.
    pub sample_rate: SampleRate,
    /// Samples per frame, interleaved.
    pub channels: Channels,
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn sample_rate_rejects_zero() {
        assert_eq!(SampleRate::new(0), None);
        assert_eq!(SampleRate::try_from(0), Err(ZeroSampleRateError));
    }

    #[test]
    fn channels_rejects_zero() {
        assert_eq!(Channels::new(0), None);
        assert_eq!(Channels::try_from(0), Err(ZeroChannelsError));
    }

    #[test]
    fn channels_round_trip_every_non_zero_count() {
        for count in 1..=u16::MAX {
            assert_eq!(Channels::new(count).map(Channels::get), Some(count));
            assert_eq!(Channels::try_from(count).map(Channels::get), Ok(count));
        }
    }

    #[test]
    fn sample_rate_round_trips_its_extremes() {
        for hz in [1, u32::MAX] {
            assert_eq!(SampleRate::new(hz).map(SampleRate::get), Some(hz));
            assert_eq!(SampleRate::try_from(hz).map(SampleRate::get), Ok(hz));
        }
    }

    #[test]
    fn zero_errors_say_what_was_zero() {
        assert_eq!(
            ZeroSampleRateError.to_string(),
            "sample rate must be non-zero"
        );
        assert_eq!(
            ZeroChannelsError.to_string(),
            "channel count must be non-zero"
        );
    }

    #[test]
    fn a_rate_of_frames_lasts_one_second() {
        let rate = SampleRate::new(44_100).expect("non-zero");
        assert_eq!(rate.duration_of(44_100), Duration::from_secs(1));
        assert_eq!(rate.duration_of(66_150), Duration::from_millis(1_500));
    }

    proptest! {
        #[test]
        #[cfg_attr(miri, ignore = "proptest reads its regression file, which Miri's isolation forbids")]
        fn duration_of_is_the_exact_quotient_rounded_down(frames: u64, hz in 1..=u32::MAX) {
            let rate = SampleRate::new(hz).expect("the strategy excludes zero");
            let exact = u128::from(frames) * 1_000_000_000 / u128::from(hz);
            prop_assert_eq!(rate.duration_of(frames).as_nanos(), exact);
        }

        #[test]
        #[cfg_attr(miri, ignore = "proptest reads its regression file, which Miri's isolation forbids")]
        fn sample_rate_round_trips_every_non_zero_rate(hz in 1..=u32::MAX) {
            prop_assert_eq!(SampleRate::new(hz).map(SampleRate::get), Some(hz));
            prop_assert_eq!(SampleRate::try_from(hz).map(SampleRate::get), Ok(hz));
        }
    }
}
