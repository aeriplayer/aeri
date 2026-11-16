//! An in-memory [`OutputSink`] for tests.

use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use aeri_core::StreamSpec;

use crate::{OutputSink, SinkError};

/// An [`OutputSink`] that plays into memory, for testing code that drives a sink.
///
/// The fake device plays everything buffered whenever the sink is used while not paused:
/// at the start of each [`write`](OutputSink::write), and in
/// [`frames_played`](OutputSink::frames_played) and [`is_drained`](OutputSink::is_drained).
/// Back-pressure is therefore visible within one `write` larger than the capacity, and
/// across writes while paused.
///
/// Inspect what it did through a [`FakeSinkObserver`], which stays with the test after
/// the sink itself has been moved into the code under test. The observer also sets how much
/// the device may play ([`hold`](FakeSinkObserver::hold), [`allow`](FakeSinkObserver::allow),
/// [`release`](FakeSinkObserver::release)), so a test decides when time passes.
#[derive(Debug)]
pub struct FakeSink {
    capacity: NonZeroUsize,
    negotiate: Option<StreamSpec>,
    // Shared with `FakeSinkObserver` on the test thread: small, uncontended, never real-time.
    state: Arc<Mutex<State>>,
}

impl FakeSink {
    /// A sink that buffers at most `capacity` frames and negotiates whatever is requested.
    #[must_use]
    pub fn new(capacity: NonZeroUsize) -> Self {
        Self {
            capacity,
            negotiate: None,
            state: Arc::default(),
        }
    }

    /// Makes every [`configure`](OutputSink::configure) negotiate `spec`, whatever was
    /// requested.
    #[must_use]
    pub fn negotiating(mut self, spec: StreamSpec) -> Self {
        self.negotiate = Some(spec);
        self
    }

    /// A handle for inspecting this sink after it has been handed away.
    #[must_use]
    pub fn observer(&self) -> FakeSinkObserver {
        FakeSinkObserver {
            state: Arc::clone(&self.state),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }
}

impl OutputSink for FakeSink {
    fn configure(&mut self, requested: StreamSpec) -> Result<StreamSpec, SinkError> {
        let negotiated = self.negotiate.unwrap_or(requested);
        self.state().configure(requested, negotiated);
        Ok(negotiated)
    }

    fn write(&mut self, interleaved: &[f32]) -> Result<usize, SinkError> {
        let capacity = self.capacity.get();
        self.state().write(interleaved, capacity)
    }

    fn set_paused(&mut self, paused: bool) {
        self.state().paused = paused;
    }

    fn flush(&mut self) {
        self.state().flush();
    }

    fn frames_played(&self) -> u64 {
        self.state().play().frames_played
    }

    fn is_drained(&self) -> bool {
        self.state().play().buffered.is_empty()
    }
}

/// Inspects a [`FakeSink`] from the test that created it.
#[derive(Debug, Clone)]
pub struct FakeSinkObserver {
    state: Arc<Mutex<State>>,
}

impl FakeSinkObserver {
    /// Every sample the fake device has played, interleaved, in order, across every
    /// `configure` and `flush`.
    #[must_use]
    pub fn played(&self) -> Vec<f32> {
        lock(&self.state).played.clone()
    }

    /// The samples played since the last `flush` or `configure`.
    #[must_use]
    pub fn played_since_flush(&self) -> Vec<f32> {
        let state = lock(&self.state);
        state
            .played
            .get(state.played_at_flush..)
            .unwrap_or_default()
            .to_vec()
    }

    /// The `configure` and `flush` calls made so far, in order.
    #[must_use]
    pub fn calls(&self) -> Vec<SinkCall> {
        lock(&self.state).calls.clone()
    }

    /// Stops the device pulling audio, as if it had stalled. Nothing is played until
    /// [`allow`](Self::allow) or [`release`](Self::release).
    pub fn hold(&self) {
        lock(&self.state).budget = Some(0);
    }

    /// Lets the device play up to `frames` more frames, then hold again.
    pub fn allow(&self, frames: u64) {
        lock(&self.state).budget = Some(frames);
    }

    /// Lets the device play without limit, the default.
    pub fn release(&self) {
        lock(&self.state).budget = None;
    }
}

/// A call made to a [`FakeSink`], as recorded by [`FakeSinkObserver::calls`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SinkCall {
    /// [`OutputSink::configure`], with the spec asked for and the spec returned.
    Configure {
        /// The spec passed in.
        requested: StreamSpec,
        /// The spec returned.
        negotiated: StreamSpec,
    },
    /// [`OutputSink::flush`].
    Flush,
}

#[derive(Debug, Default)]
struct State {
    spec: Option<StreamSpec>,
    paused: bool,
    buffered: Vec<f32>,
    played: Vec<f32>,
    played_at_flush: usize,
    frames_played: u64,
    calls: Vec<SinkCall>,
    /// Frames the device may still play; `None` is unlimited.
    budget: Option<u64>,
}

impl State {
    fn channels(&self) -> Option<usize> {
        self.spec.map(|spec| usize::from(spec.channels.get()))
    }

    fn configure(&mut self, requested: StreamSpec, negotiated: StreamSpec) {
        self.calls.push(SinkCall::Configure {
            requested,
            negotiated,
        });
        self.spec = Some(negotiated);
        self.buffered.clear();
        self.played_at_flush = self.played.len();
        self.frames_played = 0;
    }

    fn write(&mut self, interleaved: &[f32], capacity: usize) -> Result<usize, SinkError> {
        let channels = self.channels().ok_or(SinkError::NotConfigured)?;
        self.play();
        let free = capacity.saturating_sub(self.buffered.len() / channels);
        let frames = (interleaved.len() / channels).min(free);
        self.buffered.extend(
            interleaved
                .chunks_exact(channels)
                .take(frames)
                .flatten()
                .copied(),
        );
        Ok(frames)
    }

    fn flush(&mut self) {
        self.calls.push(SinkCall::Flush);
        self.buffered.clear();
        self.played_at_flush = self.played.len();
        self.frames_played = 0;
    }

    /// Plays everything buffered that the budget allows, unless paused.
    fn play(&mut self) -> &mut Self {
        if !self.paused
            && let Some(channels) = self.channels()
        {
            let buffered = (self.buffered.len() / channels) as u64;
            let frames = self.budget.map_or(buffered, |budget| budget.min(buffered));
            if let Some(budget) = &mut self.budget {
                *budget -= frames;
            }
            let samples = usize::try_from(frames).unwrap_or(usize::MAX) * channels;
            self.played.extend(self.buffered.drain(..samples));
            self.frames_played += frames;
        }
        self
    }
}

// A test that panicked mid-call poisons the mutex; its state is still worth inspecting.
fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use aeri_core::{Channels, SampleRate};

    use super::*;

    fn stereo(hz: u32) -> StreamSpec {
        StreamSpec {
            sample_rate: SampleRate::new(hz).expect("test rates are non-zero"),
            channels: Channels::new(2).expect("2 is non-zero"),
        }
    }

    fn configured(capacity: usize) -> (FakeSink, FakeSinkObserver) {
        let capacity = NonZeroUsize::new(capacity).expect("test capacities are non-zero");
        let mut sink = FakeSink::new(capacity);
        sink.configure(stereo(44_100))
            .expect("the fake always configures");
        let observer = sink.observer();
        (sink, observer)
    }

    fn ramp(samples: u16) -> Vec<f32> {
        (0..samples).map(f32::from).collect()
    }

    #[test]
    fn write_before_configure_is_refused() {
        let mut sink = FakeSink::new(NonZeroUsize::MIN);

        assert!(matches!(
            sink.write(&[0.0, 0.0]),
            Err(SinkError::NotConfigured)
        ));
        assert_eq!(sink.observer().played(), []);
    }

    #[test]
    fn configure_negotiates_the_requested_spec_by_default() {
        let mut sink = FakeSink::new(NonZeroUsize::MIN);

        assert_eq!(sink.configure(stereo(48_000)).ok(), Some(stereo(48_000)));
        assert_eq!(
            sink.observer().calls(),
            [SinkCall::Configure {
                requested: stereo(48_000),
                negotiated: stereo(48_000),
            }]
        );
    }

    #[test]
    fn configure_negotiates_the_overridden_spec_when_set() {
        let mut sink = FakeSink::new(NonZeroUsize::MIN).negotiating(stereo(48_000));

        assert_eq!(sink.configure(stereo(44_100)).ok(), Some(stereo(48_000)));
        assert_eq!(
            sink.observer().calls(),
            [SinkCall::Configure {
                requested: stereo(44_100),
                negotiated: stereo(48_000),
            }]
        );
    }

    #[test]
    fn write_accepts_no_more_than_capacity() {
        let (mut sink, observer) = configured(4);

        assert_eq!(sink.write(&ramp(20)).ok(), Some(4));
        assert_eq!(sink.frames_played(), 4);
        assert_eq!(observer.played(), ramp(8));
    }

    #[test]
    fn write_accepts_whole_frames_only() {
        let (mut sink, observer) = configured(8);

        assert_eq!(sink.write(&ramp(5)).ok(), Some(2));
        assert_eq!(sink.frames_played(), 2);
        assert_eq!(observer.played(), ramp(4));
    }

    #[test]
    fn a_full_buffer_accepts_nothing_while_paused() {
        let (mut sink, _) = configured(2);
        sink.set_paused(true);

        assert_eq!(sink.write(&ramp(4)).ok(), Some(2));
        assert_eq!(sink.write(&ramp(4)).ok(), Some(0));
    }

    #[test]
    fn a_partly_full_buffer_accepts_only_the_space_left() {
        let (mut sink, _) = configured(3);
        sink.set_paused(true);

        assert_eq!(sink.write(&ramp(4)).ok(), Some(2));
        assert_eq!(sink.write(&ramp(4)).ok(), Some(1));
    }

    #[test]
    fn writing_after_resume_plays_what_was_buffered_first() {
        let (mut sink, observer) = configured(2);
        sink.set_paused(true);
        sink.write(&[1.0, 1.0, 2.0, 2.0]).expect("configured");
        sink.set_paused(false);

        assert_eq!(sink.write(&[3.0, 3.0]).ok(), Some(1));
        assert_eq!(observer.played(), [1.0, 1.0, 2.0, 2.0]);
    }

    #[test]
    fn pausing_holds_frames_played_and_resuming_loses_nothing() {
        let (mut sink, observer) = configured(8);
        sink.write(&ramp(4)).expect("configured");
        assert_eq!(sink.frames_played(), 2);

        sink.set_paused(true);
        sink.write(&ramp(6)).expect("configured");
        assert_eq!(sink.frames_played(), 2);
        assert!(!sink.is_drained());

        sink.set_paused(false);
        assert!(sink.is_drained());
        assert_eq!(sink.frames_played(), 5);
        assert_eq!(observer.played(), [ramp(4), ramp(6)].concat());
    }

    #[test]
    fn flush_discards_buffered_audio_and_resets_frames_played() {
        let (mut sink, observer) = configured(8);
        sink.write(&ramp(4)).expect("configured");
        assert_eq!(sink.frames_played(), 2);
        sink.set_paused(true);
        sink.write(&ramp(6)).expect("configured");

        sink.flush();
        sink.set_paused(false);

        assert_eq!(sink.frames_played(), 0);
        assert!(sink.is_drained());
        assert_eq!(observer.played(), ramp(4));
        assert_eq!(observer.calls().last(), Some(&SinkCall::Flush));
    }

    #[test]
    fn a_held_device_plays_nothing_until_allowed_and_then_only_the_allowance() {
        let (mut sink, observer) = configured(8);
        observer.hold();
        sink.write(&ramp(8)).expect("configured");
        assert_eq!(sink.frames_played(), 0);
        assert!(!sink.is_drained());

        observer.allow(3);
        assert_eq!(sink.frames_played(), 3);
        assert_eq!(sink.frames_played(), 3);
        assert_eq!(observer.played(), ramp(6));

        observer.release();
        assert!(sink.is_drained());
        assert_eq!(sink.frames_played(), 4);
    }

    #[test]
    fn played_since_flush_starts_over_at_each_flush() {
        let (mut sink, observer) = configured(8);
        sink.write(&ramp(4)).expect("configured");
        assert_eq!(sink.frames_played(), 2);
        sink.flush();
        assert_eq!(observer.played_since_flush(), []);

        sink.write(&[7.0, 7.0]).expect("configured");
        assert_eq!(sink.frames_played(), 1);
        assert_eq!(observer.played_since_flush(), [7.0, 7.0]);
        assert_eq!(observer.played().len(), 6);
    }

    #[test]
    fn reconfiguring_discards_buffered_audio_and_resets_frames_played() {
        let (mut sink, observer) = configured(8);
        sink.write(&ramp(4)).expect("configured");
        assert_eq!(sink.frames_played(), 2);
        sink.set_paused(true);
        sink.write(&ramp(6)).expect("configured");

        sink.configure(stereo(48_000))
            .expect("the fake always configures");
        sink.set_paused(false);

        assert_eq!(sink.frames_played(), 0);
        assert_eq!(observer.played(), ramp(4));
        assert_eq!(observer.calls().len(), 2);
    }
}
