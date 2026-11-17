//! The playback thread: the one owner of playback state (ADR 0004).

use std::collections::VecDeque;
use std::mem;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use aeri_audio::OutputSink;
use aeri_core::{PlaybackState, ReplayGainMode, SampleRate, StreamSpec, Track, Transport, Volume};
use aeri_decode::Decoder;
use aeri_dsp::{Chain, Control, DEFAULT_CHAIN, Registry, TrackStart};
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::{mpsc, oneshot, watch};

use crate::{EngineError, EngineEvent, Opener};

/// Position is published at most this often; transitions are published at once.
const PUBLISH_EVERY: Duration = Duration::from_millis(100);
/// How long to wait before offering audio again to a full or draining output.
const BACKOFF: Duration = Duration::from_millis(5);

/// An error and its causes, innermost last: what a log line should say, since the file name
/// and the reason are in the sources, not in the top-level message.
fn chain(error: &(dyn std::error::Error + 'static)) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

type Reply = oneshot::Sender<Result<(), EngineError>>;
/// Hands an event to the owner. Boxed so the engine need not know the owner's message type.
pub(crate) type Emit = Box<dyn FnMut(EngineEvent) + Send>;

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
    SetVolume {
        volume: Volume,
        reply: Reply,
    },
    SetReplayGain {
        mode: ReplayGainMode,
        reply: Reply,
    },
}

pub(crate) struct Playback {
    sink: Box<dyn OutputSink>,
    commands: mpsc::Receiver<Command>,
    state: watch::Sender<PlaybackState>,
    events: Emit,
    open: Opener,
    chain: Chain,
    /// The stream spec the sink is open for, and the spec it negotiated.
    opened: Option<(StreamSpec, StreamSpec)>,
    /// The track taken when the current one ends, already opened (ADR 0027).
    next: Option<(Arc<Track>, Box<dyn Decoder>)>,
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
    /// Tracks taken from the slot, with the sink frame they start at, whose `NowPlaying` is due.
    announce: VecDeque<(u64, Arc<Track>)>,
    /// A track already announced by `TookNext` that a seek pulled back from a pending reopen.
    deferred: Option<(Arc<Track>, Box<dyn Decoder>)>,
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
    fn new(
        track: Arc<Track>,
        decoder: Box<dyn Decoder>,
        position: Duration,
        announce: bool,
    ) -> Self {
        let rate = decoder.spec().sample_rate;
        let announce = if announce {
            VecDeque::from([(0, Arc::clone(&track))])
        } else {
            VecDeque::new()
        };
        Self {
            track,
            decoder,
            start: Segment {
                frame: 0,
                position,
                rate,
            },
            previous: None,
            announce,
            deferred: None,
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
        events: Emit,
        open: Opener,
    ) -> Self {
        let chain = Registry::builtin()
            .build(DEFAULT_CHAIN)
            .expect("the default chain names only built-in nodes");
        Self {
            sink,
            commands,
            state,
            events,
            open,
            chain,
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
        self.flush();
        self.active = None;
        self.refresh();
    }

    /// Discards what the output holds and what the chain remembers.
    fn flush(&mut self) {
        self.sink.flush();
        self.chain.reset();
    }

    fn handle(&mut self, command: Command) {
        let (reply, result) = match command {
            Command::Play { track, reply } => (reply, self.play(track)),
            Command::SetNext { track, reply } => {
                self.set_next(track);
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
                    self.flush();
                    self.sink.set_paused(false);
                }
                (reply, Ok(()))
            }
            Command::Seek { to, reply } => (reply, self.seek(to)),
            Command::SetVolume { volume, reply } => {
                self.chain.control(&Control::Volume(volume));
                (reply, Ok(()))
            }
            Command::SetReplayGain { mode, reply } => {
                self.chain.control(&Control::ReplayGain(mode));
                (reply, Ok(()))
            }
        };
        self.refresh();
        if reply.send(result).is_err() {
            tracing::debug!("a caller stopped waiting for its reply");
        }
    }

    /// Fills or clears the slot. A file that cannot be opened is reported at once and leaves the
    /// slot empty, so the current track plays on and the owner can offer another (ADR 0027).
    fn set_next(&mut self, track: Option<Arc<Track>>) {
        self.next = track.and_then(|track| match (self.open)(&track.source) {
            Ok(decoder) => Some((track, decoder)),
            Err(error) => {
                tracing::warn!(
                    file = %track.source,
                    reason = %chain(&error),
                    "the next track cannot be opened"
                );
                (self.events)(EngineEvent::Failed(track));
                None
            }
        });
    }

    fn play(&mut self, track: Arc<Track>) -> Result<(), EngineError> {
        let decoder = (self.open)(&track.source)?;
        self.next = None;
        self.start(track, decoder, false)
    }

    /// Makes `track` current, reusing the open output when the spec allows.
    fn start(
        &mut self,
        track: Arc<Track>,
        decoder: Box<dyn Decoder>,
        announce: bool,
    ) -> Result<(), EngineError> {
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
        self.chain.reset();
        self.chain
            .start_track(&TrackStart::new(spec, decoder.replay_gain()));
        self.sink.set_paused(false);
        self.active = Some(Active::new(track, decoder, Duration::ZERO, announce));
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
        self.chain.reset();
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
        for (frame, _) in &mut active.announce {
            *frame = 0;
        }
        if let End::Reopen(next, decoder) = mem::replace(&mut active.end, End::NotYet) {
            active.deferred = Some((next, decoder));
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
                    self.chain
                        .process(&mut active.block, active.decoder.spec().channels);
                }
                Ok(None) => return self.end_of_track(),
                Err(error) => {
                    tracing::error!(
                        file = %active.track.source,
                        reason = %chain(&error),
                        "decoding failed; skipping the rest of the track"
                    );
                    (self.events)(EngineEvent::Failed(Arc::clone(&active.track)));
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
                self.ended();
            }
        }
    }

    /// The current decoder is exhausted: take the next slot (ADR 0005).
    fn end_of_track(&mut self) {
        let Some(active) = &mut self.active else {
            return;
        };
        let taken = match active.deferred.take() {
            Some(taken) => Some(taken),
            None => self.next.take().inspect(|(track, _)| {
                (self.events)(EngineEvent::TookNext(Arc::clone(track)));
            }),
        };
        let Some((next, decoder)) = taken else {
            active.end = End::Stop;
            return;
        };
        if decoder.spec() == active.decoder.spec() {
            self.chain
                .start_track(&TrackStart::new(decoder.spec(), decoder.replay_gain()));
            let start = Segment {
                frame: active.written,
                position: Duration::ZERO,
                rate: decoder.spec().sample_rate,
            };
            active
                .announce
                .push_back((active.written, Arc::clone(&next)));
            let finished = mem::replace(&mut active.track, next);
            active.previous = Some((finished, mem::replace(&mut active.start, start)));
            active.decoder = decoder;
            active.block.clear();
            active.offset = 0;
        } else {
            active.end = End::Reopen(next, decoder);
        }
    }

    /// Playback stopped on its own. The state says so before the event does, so an owner
    /// that checks the state on receiving it never sees `Active` (ADR 0025).
    fn ended(&mut self) {
        self.refresh();
        (self.events)(EngineEvent::Ended);
    }

    /// Waits for the output to play out what it holds, then stops or reopens it.
    fn drain(&mut self) {
        if !self.sink.is_drained() {
            thread::sleep(BACKOFF);
            return;
        }
        self.announce(None);
        let Some(active) = self.active.take() else {
            return;
        };
        match active.end {
            End::Reopen(track, decoder) => {
                let reopened = Arc::clone(&track);
                if let Err(error) = self.start(track, decoder, true) {
                    tracing::error!(%error, "the output cannot be reopened for the next track; stopping");
                    (self.events)(EngineEvent::Failed(reopened));
                    self.ended();
                }
            }
            End::Stop | End::NotYet => self.ended(),
        }
    }

    /// Emits `NowPlaying` for each taken track whose first frame the output has reached
    /// (`played`), or for all of them when `played` is `None`.
    fn announce(&mut self, played: Option<u64>) {
        let Some(active) = &mut self.active else {
            return;
        };
        while let Some((frame, _)) = active.announce.front()
            && played.is_none_or(|played| *frame <= played)
            && let Some((_, track)) = active.announce.pop_front()
        {
            (self.events)(EngineEvent::NowPlaying(track));
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
        self.announce(Some(played));
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

    #[derive(Debug, thiserror::Error)]
    #[error("the disk is full")]
    struct Inner;

    #[derive(Debug, thiserror::Error)]
    #[error("cannot write")]
    struct Middle(#[source] Inner);

    #[derive(Debug, thiserror::Error)]
    #[error("cannot save the file")]
    struct Outer(#[source] Middle);

    #[test]
    fn a_logged_error_names_every_cause_outermost_first() {
        assert_eq!(
            chain(&Outer(Middle(Inner))),
            "cannot save the file: cannot write: the disk is full"
        );
    }

    #[test]
    fn an_error_without_a_cause_is_just_its_message() {
        assert_eq!(chain(&Inner), "the disk is full");
    }

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
