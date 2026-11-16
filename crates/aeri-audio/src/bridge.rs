//! The hand-off between a device sink and its real-time callback (ADR 0016).
//!
//! The sink owns a [`Writer`]; the platform callback owns a [`Renderer`]. Between them are
//! a single-producer, single-consumer ring of interleaved samples and four `Relaxed`
//! atomics, each written by one side only. Positions count samples since the bridge was
//! built, with discarded samples counted as consumed.

use std::num::NonZeroU64;

use aeri_core::StreamSpec;

use crate::sync::{Arc, AtomicBool, AtomicU64, Ordering};

/// How much audio a device sink's ring holds: a fifth of a second (ADR 0016).
const BUFFERED_SECONDS_RECIPROCAL: usize = 5;

/// The producer end of a sample ring.
pub(crate) trait RingWriter: Send {
    /// Samples that can be pushed now.
    fn free(&self) -> usize;

    /// Pushes as many of `samples` as fit; returns how many.
    fn push(&mut self, samples: &[f32]) -> usize;
}

/// The consumer end of a sample ring. Every method is real-time safe.
pub(crate) trait RingReader: Send {
    /// Samples that can be popped now.
    fn available(&self) -> usize;

    /// Pops into the front of `out`, as many as are available and fit; returns how many.
    fn pop_into(&mut self, out: &mut [f32]) -> usize;

    /// Discards up to `count` samples; returns how many.
    fn skip(&mut self, count: usize) -> usize;
}

impl RingWriter for rtrb::Producer<f32> {
    fn free(&self) -> usize {
        self.slots()
    }

    fn push(&mut self, samples: &[f32]) -> usize {
        let count = samples.len().min(self.slots());
        self.write_chunk_uninit(count)
            .map_or(0, |chunk| chunk.fill_from_iter(samples.iter().copied()))
    }
}

impl RingReader for rtrb::Consumer<f32> {
    fn available(&self) -> usize {
        self.slots()
    }

    fn pop_into(&mut self, out: &mut [f32]) -> usize {
        let count = out.len().min(self.slots());
        let Ok(chunk) = self.read_chunk(count) else {
            return 0;
        };
        let (first, second) = chunk.as_slices();
        for (slot, sample) in out.iter_mut().zip(first.iter().chain(second)) {
            *slot = *sample;
        }
        chunk.commit_all();
        count
    }

    fn skip(&mut self, count: usize) -> usize {
        let count = count.min(self.slots());
        self.read_chunk(count).map_or(0, |chunk| {
            chunk.commit_all();
            count
        })
    }
}

#[derive(Debug)]
struct Shared {
    paused: AtomicBool,
    /// Written by the [`Writer`]: nothing before this position may be played.
    flush_at: AtomicU64,
    /// Written by the [`Renderer`]: the position of the next sample it will consume.
    read_pos: AtomicU64,
    /// Written by the [`Renderer`]: buffers it could not fill.
    underruns: AtomicU64,
}

/// Builds a bridge over a ring for `channels`-channel audio.
pub(crate) fn bridge<W: RingWriter, R: RingReader>(
    (writer, reader): (W, R),
    channels: usize,
) -> (Writer<W>, Renderer<R>) {
    let shared = Arc::new(Shared {
        paused: AtomicBool::new(false),
        flush_at: AtomicU64::new(0),
        read_pos: AtomicU64::new(0),
        underruns: AtomicU64::new(0),
    });
    let channels = channels.max(1);
    (
        Writer {
            ring: writer,
            shared: Arc::clone(&shared),
            channels,
            written: 0,
            flush_at: 0,
            underruns_seen: 0,
        },
        Renderer {
            ring: reader,
            shared,
            channels,
            read_pos: 0,
        },
    )
}

/// Builds a device sink's bridge: an `rtrb` ring holding 200 ms of `spec` (ADR 0016).
pub(crate) fn device_bridge(
    spec: StreamSpec,
) -> (Writer<rtrb::Producer<f32>>, Renderer<rtrb::Consumer<f32>>) {
    let channels = usize::from(spec.channels.get());
    let capacity = spec.sample_rate.get() as usize / BUFFERED_SECONDS_RECIPROCAL * channels;
    bridge(rtrb::RingBuffer::new(capacity), channels)
}

/// The sink's half of the bridge.
#[derive(Debug)]
pub(crate) struct Writer<W> {
    ring: W,
    shared: Arc<Shared>,
    channels: usize,
    written: u64,
    flush_at: u64,
    underruns_seen: u64,
}

impl<W: RingWriter> Writer<W> {
    /// Pushes as many whole frames of `interleaved` as fit; returns how many frames.
    pub(crate) fn write(&mut self, interleaved: &[f32]) -> usize {
        let frames = (interleaved.len() / self.channels).min(self.ring.free() / self.channels);
        let pushed = self.ring.push(
            interleaved
                .get(..frames * self.channels)
                .unwrap_or_default(),
        );
        self.written += pushed as u64;
        pushed / self.channels
    }

    /// Makes everything written so far unplayable. New audio may be written at once.
    pub(crate) fn flush(&mut self) {
        self.flush_at = self.written;
        self.shared.flush_at.store(self.written, Ordering::Relaxed);
    }

    pub(crate) fn set_paused(&self, paused: bool) {
        self.shared.paused.store(paused, Ordering::Relaxed);
    }

    /// Frames the callback has played since the bridge was built or last flushed.
    pub(crate) fn frames_played(&self) -> u64 {
        let read_pos = self.shared.read_pos.load(Ordering::Relaxed);
        read_pos.saturating_sub(self.flush_at) / self.channels as u64
    }

    /// True once the callback has consumed everything written.
    pub(crate) fn is_drained(&self) -> bool {
        self.shared.read_pos.load(Ordering::Relaxed) >= self.written
    }

    /// Underruns the callback has counted since this was last called, if there were any.
    pub(crate) fn take_underruns(&mut self) -> Option<NonZeroU64> {
        let total = self.shared.underruns.load(Ordering::Relaxed);
        let new = total.saturating_sub(self.underruns_seen);
        self.underruns_seen = total;
        NonZeroU64::new(new)
    }
}

/// The callback's half of the bridge. [`render`](Self::render) is the callback body.
#[derive(Debug)]
pub(crate) struct Renderer<R> {
    ring: R,
    shared: Arc<Shared>,
    channels: usize,
    read_pos: u64,
}

impl<R: RingReader> Renderer<R> {
    /// Fills `out` with interleaved samples, or silence where there are none.
    ///
    /// Real-time safe: no allocation, locking, blocking, or panicking (brief, Step 4).
    pub(crate) fn render(&mut self, out: &mut [f32]) {
        if self.shared.paused.load(Ordering::Relaxed) {
            out.fill(0.0);
            return;
        }

        // The ring's fill level must be read before `flush_at` (ADR 0016).
        let available = self.ring.available();
        let flush_at = self.shared.flush_at.load(Ordering::Relaxed);
        let stale = usize::try_from(flush_at.saturating_sub(self.read_pos)).unwrap_or(usize::MAX);
        let skipped = self.ring.skip(stale.min(available));
        let available = available.saturating_sub(skipped);
        self.read_pos = self.read_pos.saturating_add(skipped as u64);

        let wanted = out.len() - out.len() % self.channels;
        let take = available.min(wanted) / self.channels * self.channels;
        // Clamped to `out.len()`, so the split cannot panic.
        let (head, tail) = out.split_at_mut(take.min(out.len()));
        let popped = self.ring.pop_into(head);
        if let Some(unfilled) = head.get_mut(popped..) {
            unfilled.fill(0.0);
        }
        tail.fill(0.0);

        self.read_pos = self.read_pos.saturating_add(popped as u64);
        self.shared.read_pos.store(self.read_pos, Ordering::Relaxed);
        if popped < out.len() {
            self.shared.underruns.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
#[cfg(not(loom))]
mod tests {
    use super::*;
    use crate::alloc_guard::assert_no_alloc;

    fn stereo(
        capacity_frames: usize,
    ) -> (Writer<rtrb::Producer<f32>>, Renderer<rtrb::Consumer<f32>>) {
        bridge(rtrb::RingBuffer::new(capacity_frames * 2), 2)
    }

    fn ramp(from: u16, samples: u16) -> Vec<f32> {
        (from..from + samples).map(f32::from).collect()
    }

    #[test]
    fn a_full_ring_fills_the_buffer_in_order() {
        let (mut writer, mut renderer) = stereo(8);
        writer.write(&ramp(1, 8));
        let mut out = [9.0; 8];

        assert_no_alloc(|| renderer.render(&mut out));

        assert_eq!(out.to_vec(), ramp(1, 8));
        assert_eq!(writer.frames_played(), 4);
        assert!(writer.is_drained());
        assert_eq!(writer.take_underruns(), None);
    }

    #[test]
    fn an_underrun_plays_what_there_is_then_silence_and_is_counted() {
        let (mut writer, mut renderer) = stereo(8);
        writer.write(&ramp(1, 4));
        let mut out = [9.0; 8];

        assert_no_alloc(|| renderer.render(&mut out));

        assert_eq!(out, [1.0, 2.0, 3.0, 4.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(writer.frames_played(), 2);
        assert_eq!(writer.take_underruns(), NonZeroU64::new(1));
        assert_eq!(writer.take_underruns(), None);
    }

    #[test]
    fn pausing_plays_silence_and_consumes_nothing() {
        let (mut writer, mut renderer) = stereo(8);
        writer.write(&ramp(1, 4));
        writer.set_paused(true);
        let mut out = [9.0; 4];

        assert_no_alloc(|| renderer.render(&mut out));
        assert_eq!(out, [0.0; 4]);
        assert_eq!(writer.frames_played(), 0);
        assert!(!writer.is_drained());
        assert_eq!(writer.take_underruns(), None);

        writer.set_paused(false);
        assert_no_alloc(|| renderer.render(&mut out));
        assert_eq!(out.to_vec(), ramp(1, 4));
    }

    #[test]
    fn a_flush_discards_buffered_audio_but_not_what_follows_it() {
        let (mut writer, mut renderer) = stereo(8);
        writer.write(&ramp(1, 4));
        writer.flush();
        writer.write(&ramp(100, 4));
        let mut out = [9.0; 4];

        assert_no_alloc(|| renderer.render(&mut out));

        assert_eq!(out.to_vec(), ramp(100, 4));
        assert_eq!(writer.frames_played(), 2);
        assert!(writer.is_drained());
    }

    #[test]
    fn frames_played_restarts_at_a_flush() {
        let (mut writer, mut renderer) = stereo(8);
        writer.write(&ramp(1, 4));
        let mut out = [0.0; 4];
        renderer.render(&mut out);
        assert_eq!(writer.frames_played(), 2);

        writer.flush();
        assert_eq!(writer.frames_played(), 0);
        writer.write(&ramp(1, 2));
        renderer.render(&mut out);
        assert_eq!(writer.frames_played(), 1);
    }

    #[test]
    fn write_accepts_whole_frames_up_to_the_free_space() {
        let (mut writer, _renderer) = stereo(2);

        assert_eq!(writer.write(&ramp(1, 3)), 1);
        assert_eq!(writer.write(&ramp(1, 6)), 1);
        assert_eq!(writer.write(&ramp(1, 2)), 0);
    }

    #[test]
    fn a_ring_with_a_partial_frame_of_space_is_never_given_part_of_a_frame() {
        let (mut writer, mut renderer) = bridge(rtrb::RingBuffer::new(3), 2);

        assert_eq!(writer.write(&ramp(1, 4)), 1);
        let mut out = [9.0; 4];
        renderer.render(&mut out);

        assert_eq!(out, [1.0, 2.0, 0.0, 0.0]);
        assert!(writer.is_drained());
    }

    #[test]
    fn an_odd_sized_device_buffer_gets_whole_frames_and_a_silent_tail() {
        let (mut writer, mut renderer) = stereo(8);
        writer.write(&ramp(1, 8));
        let mut out = [9.0; 5];

        assert_no_alloc(|| renderer.render(&mut out));

        assert_eq!(out, [1.0, 2.0, 3.0, 4.0, 0.0]);
    }
}

/// `loom` models of the flush and pause protocol over a `loom`-visible ring (ADR 0018).
#[cfg(test)]
#[cfg(loom)]
mod loom_model {
    use std::collections::VecDeque;

    use loom::sync::{Arc as LoomArc, Mutex};

    use super::*;

    /// Bounded FIFO with `rtrb`'s publication guarantees: the mutex releases on push and
    /// acquires on pop.
    #[derive(Clone)]
    struct ModelRing {
        queue: LoomArc<Mutex<VecDeque<f32>>>,
        capacity: usize,
    }

    impl ModelRing {
        fn new(capacity: usize) -> Self {
            Self {
                queue: LoomArc::new(Mutex::new(VecDeque::new())),
                capacity,
            }
        }

        fn queue(&self) -> loom::sync::MutexGuard<'_, VecDeque<f32>> {
            self.queue
                .lock()
                .expect("model threads do not panic while holding the lock")
        }
    }

    impl RingWriter for ModelRing {
        fn free(&self) -> usize {
            self.capacity - self.queue().len()
        }

        fn push(&mut self, samples: &[f32]) -> usize {
            let mut queue = self.queue();
            let count = samples.len().min(self.capacity - queue.len());
            queue.extend(samples.iter().take(count));
            count
        }
    }

    impl RingReader for ModelRing {
        fn available(&self) -> usize {
            self.queue().len()
        }

        fn pop_into(&mut self, out: &mut [f32]) -> usize {
            let mut queue = self.queue();
            let count = out.len().min(queue.len());
            for (slot, sample) in out.iter_mut().zip(queue.drain(..count)) {
                *slot = sample;
            }
            count
        }

        fn skip(&mut self, count: usize) -> usize {
            let mut queue = self.queue();
            let count = count.min(queue.len());
            queue.drain(..count);
            count
        }
    }

    /// Renders one device buffer; returns its audible samples.
    fn render(renderer: &mut Renderer<ModelRing>, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0; frames];
        renderer.render(&mut out);
        out.into_iter().filter(|&sample| sample != 0.0).collect()
    }

    #[test]
    fn a_flush_never_mixes_old_and_new_audio_and_never_loses_new() {
        loom::model(|| {
            let ring = ModelRing::new(4);
            let (mut writer, mut renderer) = bridge((ring.clone(), ring), 1);
            writer.write(&[1.0, 2.0]);

            let producer = loom::thread::spawn(move || {
                writer.flush();
                writer.write(&[10.0, 11.0]);
            });
            let mut buffers = vec![render(&mut renderer, 4)];
            producer.join().expect("the producer does not panic");
            buffers.push(render(&mut renderer, 4));

            for buffer in &buffers {
                let old = buffer.iter().any(|&sample| sample < 10.0);
                let new = buffer.iter().any(|&sample| sample >= 10.0);
                assert!(
                    !(old && new),
                    "one buffer mixed old and new audio: {buffers:?}"
                );
            }
            let played: Vec<f32> = buffers.concat();
            let new: Vec<f32> = played.iter().copied().filter(|&s| s >= 10.0).collect();
            assert_eq!(new, [10.0, 11.0], "played {played:?}");
        });
    }

    #[test]
    fn a_pause_loses_and_repeats_nothing() {
        loom::model(|| {
            let ring = ModelRing::new(4);
            let (mut writer, mut renderer) = bridge((ring.clone(), ring), 1);
            writer.write(&[1.0, 2.0]);

            let producer = loom::thread::spawn(move || {
                writer.set_paused(true);
                writer.write(&[3.0]);
                writer.set_paused(false);
            });
            let mut played = render(&mut renderer, 1);
            played.extend(render(&mut renderer, 1));
            producer.join().expect("the producer does not panic");
            played.extend(render(&mut renderer, 4));

            assert_eq!(played, [1.0, 2.0, 3.0]);
        });
    }
}
