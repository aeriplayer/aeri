//! The playback thread: the one owner of playback state (ADR 0004).

use std::mem;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use aeri_audio::OutputSink;
use aeri_core::{PlaybackState, SampleRate, StreamSpec, Track, Transport};
use aeri_decode::Decoder;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::{mpsc, oneshot, watch};

use crate::EngineError;

/// Position is published at most this often; transitions are published at once.
const PUBLISH_EVERY: Duration = Duration::from_millis(100);
/// How long to wait before offering audio again to a full or draining output.
const BACKOFF: Duration = Duration::from_millis(5);

type Reply = oneshot::Sender<Result<(), EngineError>>;

/// A request to the playback thread, with the channel its answer goes back on.
#[derive(Debug)]
pub(crate) enum Command {
    Play {
        track: Arc<Track>,
        reply: Reply,
    },
    SetNext {
        track: Option<Arc<Track>>,
        reply: Reply,
    },
    Pause(Reply),
    Resume(Reply),
    Stop(Reply),
    Seek {
        to: Duration,
        reply: Reply,
    },
}

pub(crate) struct Playback {
    sink: Box<dyn OutputSink>,
    commands: mpsc::Receiver<Command>,
    state: watch::Sender<PlaybackState>,
    /// The stream spec the sink is open for, and the spec it negotiated.
    opened: Option<(StreamSpec, StreamSpec)>,
    next: Option<Arc<Track>>,
    active: Option<Active>,
    last_published: Instant,
}

/// The track being decoded, and the one before it while that is still audible.
struct Active {
    track: Arc<Track>,
    decoder: Box<dyn Decoder>,
    /// Where `track` starts in the sink's frame count.
    start: Segment,
    /// The previous track, after a gapless change, until the sink reaches `start`.
    previous: Option<(Arc<Track>, Segment)>,
    /// The decoded block being written, and how much of it the sink has taken.
    block: Vec<f32>,
    offset: usize,
    /// Frames the sink has accepted since it was opened or flushed.
    written: u64,
    paused: bool,
    end: End,
}

/// What happens once the decoder is exhausted.
enum End {
    /// Still decoding.
    NotYet,
    /// Wait for the sink to drain, then stop.
    Stop,
    /// Wait for the sink to drain, then reopen it for the next track's spec.
    Reopen(Arc<Track>, Box<dyn Decoder>),
}

#[derive(Clone, Copy)]
struct Segment {
    frame: u64,
    position: Duration,
    rate: SampleRate,
}

impl Active {
    fn new(track: Arc<Track>, decoder: Box<dyn Decoder>, position: Duration) -> Self {
        let rate = decoder.spec().sample_rate;
        Self {
            track,
            decoder,
            start: Segment {
                frame: 0,
                position,
                rate,
            },
            previous: None,
            block: Vec::new(),
            offset: 0,
            written: 0,
            paused: false,
            end: End::NotYet,
        }
    }
}

impl Playback {
    pub(crate) fn new(
        sink: Box<dyn OutputSink>,
        commands: mpsc::Receiver<Command>,
        state: watch::Sender<PlaybackState>,
    ) -> Self {
        Self {
            sink,
            commands,
            state,
            opened: None,
            next: None,
            active: None,
            last_published: Instant::now(),
        }
    }

    /// Runs until every [`Engine`](crate::Engine) handle is gone.
    pub(crate) fn run(mut self) {
        loop {
            let idle = self.active.as_ref().is_none_or(|active| active.paused);
            let command = if idle {
                match self.commands.blocking_recv() {
                    Some(command) => Some(command),
                    None => break,
                }
            } else {
                match self.commands.try_recv() {
                    Ok(command) => Some(command),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => break,
                }
            };
            if let Some(command) = command {
                self.handle(command);
            } else {
                self.step();
                self.refresh();
            }
        }
        self.sink.flush();
        self.active = None;
        self.refresh();
    }

    fn handle(&mut self, command: Command) {
        let (reply, result) = match command {
            Command::Play { track, reply } => (reply, self.play(track)),
            Command::SetNext { track, reply } => {
                self.next = track;
                (reply, Ok(()))
            }
            Command::Pause(reply) => {
                self.set_paused(true);
                (reply, Ok(()))
            }
            Command::Resume(reply) => {
                self.set_paused(false);
                (reply, Ok(()))
            }
            Command::Stop(reply) => {
                if self.active.take().is_some() {
                    self.sink.flush();
                    self.sink.set_paused(false);
                }
                (reply, Ok(()))
            }
            Command::Seek { to, reply } => (reply, self.seek(to)),
        };
        self.refresh();
        if reply.send(result).is_err() {
            tracing::debug!("a caller stopped waiting for its reply");
        }
    }

    fn play(&mut self, track: Arc<Track>) -> Result<(), EngineError> {
        let decoder = aeri_decode::open(&track.source)?;
        self.next = None;
        self.start(track, decoder)
    }

    /// Makes `track` current, reusing the open output when the spec allows.
    fn start(&mut self, track: Arc<Track>, decoder: Box<dyn Decoder>) -> Result<(), EngineError> {
        let spec = decoder.spec();
        if self.opened.is_some_and(|(open, _)| open == spec) {
            self.sink.flush();
        } else {
            self.opened = None;
            self.active = None;
            let negotiated = self.sink.configure(spec)?;
            if negotiated != spec {
                tracing::warn!(
                    ?spec,
                    ?negotiated,
                    "the output differs from the stream and nothing converts it yet"
                );
            }
            self.opened = Some((spec, negotiated));
        }
        self.sink.set_paused(false);
        self.active = Some(Active::new(track, decoder, Duration::ZERO));
        Ok(())
    }

    fn set_paused(&mut self, paused: bool) {
        if let Some(active) = &mut self.active
            && active.paused != paused
        {
            active.paused = paused;
            self.sink.set_paused(paused);
        }
    }

    fn seek(&mut self, to: Duration) -> Result<(), EngineError> {
        let active = self.active.as_mut().ok_or(EngineError::NotPlaying)?;
        let landed = active.decoder.seek(to)?;
        self.sink.flush();
        let rate = active.decoder.spec().sample_rate;
        active.start = Segment {
            frame: 0,
            position: landed,
            rate,
        };
        active.previous = None;
        active.block.clear();
        active.offset = 0;
        active.written = 0;
        if let End::Reopen(next, _) = mem::replace(&mut active.end, End::NotYet) {
            self.next = Some(next);
        }
        Ok(())
    }

    /// Does one unit of playback work: decode a block, offer audio, or wait for a drain.
    fn step(&mut self) {
        let Some(active) = &mut self.active else {
            return;
        };
        if !matches!(active.end, End::NotYet) {
            return self.drain();
        }

        if active.offset >= active.block.len() {
            match active.decoder.next_block() {
                Ok(Some(block)) => {
                    active.block.clear();
                    active.block.extend_from_slice(block);
                    active.offset = 0;
                }
                Ok(None) => return self.end_of_track(),
                Err(error) => {
                    tracing::error!(%error, "decoding failed; skipping the rest of the track");
                    return self.end_of_track();
                }
            }
        }

        let channels = usize::from(active.decoder.spec().channels.get());
        let rest = active.block.get(active.offset..).unwrap_or_default();
        match self.sink.write(rest) {
            Ok(0) => thread::sleep(BACKOFF),
            Ok(frames) => {
                active.offset += frames * channels;
                active.written += frames as u64;
            }
            Err(error) => {
                tracing::error!(%error, "the audio output failed; stopping");
                self.active = None;
                self.opened = None;
            }
        }
    }

    /// The current decoder is exhausted: take the next slot (ADR 0005).
    fn end_of_track(&mut self) {
        let Some(active) = &mut self.active else {
            return;
        };
        let Some(next) = self.next.take() else {
            active.end = End::Stop;
            return;
        };
        match aeri_decode::open(&next.source) {
            Ok(decoder) if decoder.spec() == active.decoder.spec() => {
                let start = Segment {
                    frame: active.written,
                    position: Duration::ZERO,
                    rate: decoder.spec().sample_rate,
                };
                let finished = mem::replace(&mut active.track, next);
                active.previous = Some((finished, mem::replace(&mut active.start, start)));
                active.decoder = decoder;
                active.block.clear();
                active.offset = 0;
            }
            Ok(decoder) => active.end = End::Reopen(next, decoder),
            Err(error) => {
                tracing::error!(%error, "the next track cannot be opened; stopping after this one");
                active.end = End::Stop;
            }
        }
    }

    /// Waits for the output to play out what it holds, then stops or reopens it.
    fn drain(&mut self) {
        if !self.sink.is_drained() {
            thread::sleep(BACKOFF);
            return;
        }
        let Some(active) = self.active.take() else {
            return;
        };
        if let End::Reopen(track, decoder) = active.end
            && let Err(error) = self.start(track, decoder)
        {
            tracing::error!(%error, "the output cannot be reopened for the next track; stopping");
        }
    }

    /// Publishes the state: at once if anything but the position changed, otherwise at
    /// most every [`PUBLISH_EVERY`].
    fn refresh(&mut self) {
        let played = self.sink.frames_played();
        if let Some(active) = &mut self.active
            && played >= active.start.frame
        {
            active.previous = None;
        }
        let snapshot = self.snapshot(played);
        let due = position_due(self.last_published.elapsed());
        let published = self.state.send_if_modified(|current| {
            let publish = should_publish(current, &snapshot, due);
            if publish {
                *current = snapshot;
            }
            publish
        });
        if published {
            self.last_published = Instant::now();
        }
    }

    fn snapshot(&self, played: u64) -> PlaybackState {
        let Some(active) = &self.active else {
            return PlaybackState::Stopped;
        };
        let (track, segment) = active
            .previous
            .as_ref()
            .map_or((&active.track, active.start), |(track, segment)| {
                (track, *segment)
            });
        let transport = if active.paused {
            Transport::Paused
        } else if played > segment.frame {
            Transport::Playing
        } else {
            Transport::Buffering
        };
        PlaybackState::Active {
            track: Arc::clone(track),
            transport,
            position: segment.position
                + segment
                    .rate
                    .duration_of(played.saturating_sub(segment.frame)),
            output: self.opened.map(|(_, negotiated)| negotiated),
        }
    }
}

/// Whether a position-only change may be published, `since_last` after the last publish.
fn position_due(since_last: Duration) -> bool {
    since_last >= PUBLISH_EVERY
}

/// Whether `next` replaces `current`: at once when anything but the position differs,
/// and for the position only when an update is `due`.
fn should_publish(current: &PlaybackState, next: &PlaybackState, due: bool) -> bool {
    !same_but_for_position(current, next) || (due && current != next)
}

fn same_but_for_position(a: &PlaybackState, b: &PlaybackState) -> bool {
    match (a, b) {
        (PlaybackState::Stopped, PlaybackState::Stopped) => true,
        (
            PlaybackState::Active {
                track: track_a,
                transport: transport_a,
                output: output_a,
                ..
            },
            PlaybackState::Active {
                track: track_b,
                transport: transport_b,
                output: output_b,
                ..
            },
        ) => Arc::ptr_eq(track_a, track_b) && transport_a == transport_b && output_a == output_b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use aeri_core::{Channels, Source};

    use super::*;

    fn track() -> Arc<Track> {
        Arc::new(Track {
            source: Source::File("a.flac".into()),
            duration: None,
            title: None,
            artist: None,
            album: None,
        })
    }

    fn active(track: &Arc<Track>, transport: Transport, millis: u64) -> PlaybackState {
        PlaybackState::Active {
            track: Arc::clone(track),
            transport,
            position: Duration::from_millis(millis),
            output: None,
        }
    }

    #[test]
    fn position_updates_are_due_ten_times_a_second() {
        assert!(!position_due(Duration::from_millis(99)));
        assert!(position_due(Duration::from_millis(100)));
    }

    #[test]
    fn a_position_change_waits_until_an_update_is_due() {
        let track = track();
        let current = active(&track, Transport::Playing, 0);
        let later = active(&track, Transport::Playing, 50);

        assert!(!should_publish(&current, &later, false));
        assert!(should_publish(&current, &later, true));
        assert!(!should_publish(&current, &current, true));
    }

    #[test]
    fn anything_but_the_position_is_published_at_once() {
        let track = track();
        let current = active(&track, Transport::Playing, 0);

        assert!(should_publish(
            &current,
            &active(&track, Transport::Paused, 0),
            false
        ));
        assert!(should_publish(
            &current,
            &active(&self::track(), Transport::Playing, 0),
            false
        ));
        assert!(should_publish(&current, &PlaybackState::Stopped, false));
        assert!(should_publish(&PlaybackState::Stopped, &current, false));
        assert!(!should_publish(
            &PlaybackState::Stopped,
            &PlaybackState::Stopped,
            true
        ));

        let stereo = StreamSpec {
            sample_rate: SampleRate::new(44_100).expect("non-zero"),
            channels: Channels::new(2).expect("non-zero"),
        };
        let with_output = PlaybackState::Active {
            track: Arc::clone(&track),
            transport: Transport::Playing,
            position: Duration::ZERO,
            output: Some(stereo),
        };
        assert!(should_publish(&current, &with_output, false));
    }
}
