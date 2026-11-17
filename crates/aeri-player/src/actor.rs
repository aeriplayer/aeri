//! The player thread: the one owner of the sequence and of the engine (ADR 0025).

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use aeri_core::{PlaybackState, ReplayGainMode, Track, Transport, Volume};
use aeri_engine::{Engine, EngineError, EngineEvent};
use tokio::sync::{mpsc, watch};

use crate::block_on::block_on;
use crate::error::{PlayerError, SequenceError};
use crate::ids::Position;
use crate::message::{Command, Message, Reply};
use crate::sequence::Sequence;
use crate::state::{PlayerState, Seek};

/// The most slot offers kept while the engine has not said what became of them. One older
/// than that could be in flight only if this thread were that many messages behind.
const UNREPORTED_OFFERS: usize = 16;

/// A track handed to the engine. The engine is given a fresh `Arc`, so an event that names it
/// names this offer and no other, however often the same entry is offered.
#[derive(Debug)]
struct Offer {
    position: Position,
    track: Arc<Track>,
}

pub(crate) struct Actor {
    sequence: Sequence,
    engine: Engine,
    engine_state: watch::Receiver<PlaybackState>,
    state: watch::Sender<PlayerState>,
    volume: Volume,
    replay_gain: ReplayGainMode,
    /// Offered to the engine's slot, with no report yet of what became of them.
    offers: VecDeque<Offer>,
    /// What the engine's slot holds, as far as this thread knows.
    in_slot: Option<Position>,
    /// Taken from the slot by the engine and not yet heard, first taken first.
    taken: VecDeque<Offer>,
    /// The entry being heard.
    playing: Option<Offer>,
    /// Failures since a track last played.
    failures: usize,
}

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

fn fresh(track: &Track) -> Arc<Track> {
    Arc::new(track.clone())
}

impl Actor {
    pub(crate) fn new(
        sequence: Sequence,
        engine: Engine,
        state: watch::Sender<PlayerState>,
        volume: Volume,
        replay_gain: ReplayGainMode,
    ) -> Self {
        let engine_state = engine.subscribe();
        Self {
            sequence,
            engine,
            engine_state,
            state,
            volume,
            replay_gain,
            offers: VecDeque::new(),
            in_slot: None,
            taken: VecDeque::new(),
            playing: None,
            failures: 0,
        }
    }

    /// Runs until shutdown. Dropping `self` then drops the engine, which joins its thread.
    pub(crate) fn run(mut self, mut inbox: mpsc::UnboundedReceiver<Message>) {
        while let Some(message) = inbox.blocking_recv() {
            match message {
                Message::Command(command) => self.handle(command),
                Message::Engine(event) => {
                    self.on_event(event);
                    self.publish();
                }
                Message::Shutdown => break,
            }
        }
    }

    pub(crate) fn snapshot(
        sequence: &Sequence,
        playing: Option<Position>,
        volume: Volume,
        replay_gain: ReplayGainMode,
    ) -> PlayerState {
        PlayerState {
            playlists: sequence.playlists().iter().map(Arc::clone).collect(),
            active: sequence.active(),
            current: playing,
            queue: sequence.queue().collect(),
            order: sequence.order(),
            stop_after_current: sequence.stop_after_current(),
            volume,
            replay_gain,
        }
    }

    fn publish(&self) {
        self.state.send_replace(Self::snapshot(
            &self.sequence,
            self.playing.as_ref().map(|offer| offer.position),
            self.volume,
            self.replay_gain,
        ));
    }

    // ---- commands ----

    /// Publishes the state and then answers, so a caller that has its answer reads a state
    /// that already includes the command.
    fn answer<T>(&self, reply: Reply<T>, result: Result<T, PlayerError>) {
        self.publish();
        if reply.send(result).is_err() {
            tracing::debug!("a caller stopped waiting for its reply");
        }
    }

    /// Applies an edit to the sequence, then refills the slot, which the edit may have changed.
    fn edited<T>(
        &mut self,
        reply: Reply<T>,
        edit: impl FnOnce(&mut Sequence) -> Result<T, SequenceError>,
    ) {
        let result = edit(&mut self.sequence).map_err(PlayerError::from);
        self.refill();
        self.answer(reply, result);
    }

    fn begin_or_nothing(&mut self, target: Option<Position>) -> Result<(), PlayerError> {
        target.map_or(Ok(()), |position| self.begin(position, false))
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::CreatePlaylist { name, reply } => {
                let id = self.sequence.create_playlist(name);
                self.answer(reply, Ok(id));
            }
            Command::Append { to, tracks, reply } => {
                self.edited(reply, |sequence| sequence.append(to, tracks));
            }
            Command::Remove {
                from,
                entries,
                reply,
            } => self.edited(reply, |sequence| sequence.remove(from, &entries)),
            Command::MoveBefore {
                playlist,
                entry,
                before,
                reply,
            } => self.edited(reply, |sequence| {
                sequence.move_before(playlist, entry, before)
            }),
            Command::SetActive { playlist, reply } => {
                self.edited(reply, |sequence| sequence.set_active(playlist));
            }
            Command::Enqueue {
                playlist,
                entry,
                reply,
            } => self.edited(reply, |sequence| {
                sequence.enqueue(Position { playlist, entry })
            }),
            Command::SetOrder { order, reply } => self.edited(reply, |sequence| {
                sequence.set_order(order);
                Ok(())
            }),
            Command::SetStopAfterCurrent { stop, reply } => self.edited(reply, |sequence| {
                sequence.set_stop_after_current(stop);
                Ok(())
            }),
            Command::Play(reply) => {
                let result = self.play();
                self.answer(reply, result);
            }
            Command::PlayEntry {
                playlist,
                entry,
                reply,
            } => {
                let result = self.begin(Position { playlist, entry }, true);
                self.answer(reply, result);
            }
            Command::TogglePause(reply) => {
                let result = self.toggle_pause();
                self.answer(reply, result);
            }
            Command::Stop(reply) => {
                let result = self.stop();
                self.answer(reply, result);
            }
            Command::Next(reply) => {
                let target = self.sequence.peek_skip_next();
                let result = self.begin_or_nothing(target);
                self.answer(reply, result);
            }
            Command::Previous(reply) => {
                let target = self.sequence.peek_previous();
                let result = self.begin_or_nothing(target);
                self.answer(reply, result);
            }
            Command::Seek { seek, reply } => {
                let result = self.seek(seek);
                self.answer(reply, result);
            }
            Command::SetVolume { volume, reply } => {
                self.volume = volume;
                let result = block_on(self.engine.set_volume(volume)).map_err(Into::into);
                self.answer(reply, result);
            }
            Command::SetReplayGain { mode, reply } => {
                self.replay_gain = mode;
                let result = block_on(self.engine.set_replay_gain(mode)).map_err(Into::into);
                self.answer(reply, result);
            }
        }
    }

    fn transport(&self) -> Option<Transport> {
        match &*self.engine_state.borrow() {
            PlaybackState::Active { transport, .. } => Some(*transport),
            _ => None,
        }
    }

    fn play(&mut self) -> Result<(), PlayerError> {
        match self.transport() {
            Some(Transport::Paused) => block_on(self.engine.resume()).map_err(Into::into),
            Some(_) => Ok(()),
            None => {
                let first = self
                    .sequence
                    .peek_skip_next()
                    .or_else(|| self.first_of_active())
                    .ok_or(PlayerError::NothingToPlay)?;
                self.begin(first, false)
            }
        }
    }

    fn first_of_active(&self) -> Option<Position> {
        let list = self.sequence.playlist(self.sequence.active()?)?;
        Some(Position {
            playlist: list.id(),
            entry: list.entries().first()?.id,
        })
    }

    fn toggle_pause(&mut self) -> Result<(), PlayerError> {
        match self.transport() {
            Some(Transport::Paused) => block_on(self.engine.resume()).map_err(Into::into),
            Some(_) => block_on(self.engine.pause()).map_err(Into::into),
            None => self.play(),
        }
    }

    fn stop(&mut self) -> Result<(), PlayerError> {
        block_on(self.engine.stop())?;
        block_on(self.engine.set_next(None))?;
        self.forget_playback();
        Ok(())
    }

    fn seek(&self, seek: Seek) -> Result<(), PlayerError> {
        let position = match &*self.engine_state.borrow() {
            PlaybackState::Active { position, .. } => *position,
            _ => Duration::ZERO,
        };
        let target = match seek {
            Seek::To(to) => to,
            Seek::Forward(by) => position.saturating_add(by),
            Seek::Back(by) => position.saturating_sub(by),
        };
        block_on(self.engine.seek(target)).map_err(Into::into)
    }

    // ---- starting a track ----

    fn track_at(&self, position: Position) -> Option<Arc<Track>> {
        let list = self.sequence.playlist(position.playlist)?;
        list.get(position.entry)
            .map(|entry| Arc::clone(&entry.track))
    }

    fn failure_limit(&self) -> usize {
        self.sequence
            .active()
            .and_then(|active| self.sequence.playlist(active))
            .map_or(1, |list| list.len().max(1))
    }

    /// Starts `start` now. A track that cannot be opened is skipped for the next one the
    /// order names, until as many have failed in a row as the playlist has entries.
    fn begin(&mut self, start: Position, jump: bool) -> Result<(), PlayerError> {
        let mut position = start;
        let mut first = true;
        loop {
            if first && jump {
                self.sequence.jump_to(position)?;
            } else {
                self.sequence.take(position);
            }
            first = false;
            let track = self
                .track_at(position)
                .ok_or(SequenceError::UnknownEntry(position.entry))?;
            let offer = Offer {
                position,
                track: fresh(&track),
            };
            match block_on(self.engine.play(Arc::clone(&offer.track))) {
                Ok(()) => {
                    self.forget_playback();
                    self.playing = Some(offer);
                    self.refill();
                    return Ok(());
                }
                Err(error @ EngineError::Decode(_)) => {
                    self.failures += 1;
                    tracing::warn!(
                        file = %track.source,
                        reason = %chain(&error),
                        "skipping a track that cannot be played"
                    );
                    let next = self.sequence.peek_skip_next();
                    match next {
                        Some(next) if self.failures < self.failure_limit() => position = next,
                        _ => {
                            self.failures = 0;
                            return Err(error.into());
                        }
                    }
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    /// Drops everything known about what the engine was doing.
    fn forget_playback(&mut self) {
        self.offers.clear();
        self.taken.clear();
        self.in_slot = None;
        self.playing = None;
        self.failures = 0;
    }

    // ---- the slot ----

    fn is_active(&self) -> bool {
        self.playing.is_some() || !self.taken.is_empty()
    }

    /// Makes the engine's slot hold what the sequence says plays next.
    fn refill(&mut self) {
        if !self.is_active() {
            return;
        }
        let wanted = self.sequence.peek_next();
        if wanted == self.in_slot {
            return;
        }
        let result = match wanted {
            None => block_on(self.engine.set_next(None)).map(|()| None),
            Some(position) => match self.track_at(position) {
                Some(track) => {
                    let offer = Offer {
                        position,
                        track: fresh(&track),
                    };
                    let handed = Arc::clone(&offer.track);
                    block_on(self.engine.set_next(Some(handed))).map(|()| Some(offer))
                }
                None => return,
            },
        };
        match result {
            Ok(offer) => {
                self.in_slot = wanted;
                if let Some(offer) = offer {
                    if self.offers.len() == UNREPORTED_OFFERS {
                        self.offers.pop_front();
                    }
                    self.offers.push_back(offer);
                }
            }
            Err(error) => tracing::warn!(%error, "the engine did not take the next track"),
        }
    }

    // ---- engine events ----

    fn on_event(&mut self, event: EngineEvent) {
        match event {
            EngineEvent::TookNext(track) => self.took_next(&track),
            EngineEvent::NowPlaying(track) => self.now_playing(&track),
            EngineEvent::Failed(track) => self.failed(&track),
            EngineEvent::Ended => self.ended(),
            _ => tracing::debug!("ignoring an engine event this player does not know"),
        }
    }

    /// Removes the offer for `track` and every older one, which cannot still be taken.
    fn claim(offers: &mut VecDeque<Offer>, track: &Arc<Track>) -> Option<Offer> {
        let at = offers
            .iter()
            .position(|offer| Arc::ptr_eq(&offer.track, track))?;
        offers.drain(..at);
        offers.pop_front()
    }

    fn took_next(&mut self, track: &Arc<Track>) {
        let Some(offer) = Self::claim(&mut self.offers, track) else {
            tracing::debug!("ignoring a stale slot event");
            return;
        };
        if self.offers.is_empty() {
            self.in_slot = None;
        }
        self.sequence.take(offer.position);
        self.taken.push_back(offer);
        self.refill();
    }

    fn now_playing(&mut self, track: &Arc<Track>) {
        let Some(offer) = Self::claim(&mut self.taken, track) else {
            tracing::debug!("ignoring a stale playing event");
            return;
        };
        self.playing = Some(offer);
        self.failures = 0;
    }

    fn failed(&mut self, track: &Arc<Track>) {
        if let Some(offer) = Self::claim(&mut self.offers, track) {
            if self.offers.is_empty() {
                self.in_slot = None;
            }
            self.sequence.take(offer.position);
            self.failures += 1;
        } else if self.is_heard_or_taken(track) {
            self.failures += 1;
        } else {
            tracing::debug!("ignoring a stale failure event");
        }
    }

    fn is_heard_or_taken(&self, track: &Arc<Track>) -> bool {
        self.playing
            .iter()
            .chain(&self.taken)
            .any(|offer| Arc::ptr_eq(&offer.track, track))
    }

    /// Playback stopped on its own. If the sequence has something after all (the slot could
    /// not be opened, or was filled too late), play it; otherwise stop for good.
    fn ended(&mut self) {
        if !self.is_active() || self.transport().is_some() {
            tracing::debug!("ignoring a stale end event");
            return;
        }
        self.forget_playback_keeping_failures();
        if let Some(next) = self.sequence.peek_next() {
            if let Err(error) = self.begin(next, false) {
                tracing::warn!(%error, "cannot carry on after the track ended");
            }
        } else {
            self.sequence.stopped();
            self.failures = 0;
        }
    }

    fn forget_playback_keeping_failures(&mut self) {
        let failures = self.failures;
        self.forget_playback();
        self.failures = failures;
    }
}

#[cfg(test)]
#[cfg(feature = "test-util")]
mod tests;
