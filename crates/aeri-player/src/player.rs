//! The handle frontends use (ADR 0006, ADR 0025).

use std::collections::hash_map::RandomState;
use std::hash::BuildHasher as _;
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use aeri_core::{PlaybackState, ReplayGainMode, Track, Volume};
use aeri_engine::{Engine, EngineError};
use tokio::sync::{mpsc, oneshot, watch};

use crate::actor::Actor;
use crate::error::PlayerError;
use crate::ids::{EntryId, PlaylistId};
use crate::message::{Command, Message, Reply};
use crate::sequence::{PlaybackOrder, Sequence};
use crate::state::{PlayerState, Seek};

/// A cheap, cloneable handle to the player thread.
///
/// When the last handle is dropped the thread stops playback and exits, and the drop waits for
/// it. There is no shutdown method to forget.
#[derive(Debug, Clone)]
pub struct Player {
    inner: Arc<Inner>,
    state: watch::Receiver<PlayerState>,
    playback: watch::Receiver<PlaybackState>,
}

#[derive(Debug)]
struct Inner {
    inbox: mpsc::UnboundedSender<Message>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        // The engine holds a sender too, so the inbox only closes because of this message.
        if self.inbox.send(Message::Shutdown).is_err() {
            tracing::debug!("the player thread had already exited");
        }
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::error!("the player thread panicked");
        }
    }
}

impl Player {
    /// Starts a player on this platform's default output device.
    ///
    /// # Errors
    ///
    /// [`PlayerError::Engine`] when the engine cannot start, [`PlayerError::Spawn`] when the
    /// operating system refuses a thread.
    pub fn with_default_output() -> Result<Self, PlayerError> {
        Self::start(Engine::with_default_output, RandomState::new().hash_one(()))
    }

    /// Starts a player over an in-memory sink, with a given shuffle seed, for tests.
    ///
    /// # Errors
    ///
    /// As for [`with_default_output`](Self::with_default_output).
    #[cfg(feature = "test-util")]
    pub fn with_fake_sink(sink: aeri_engine::FakeSink, seed: u64) -> Result<Self, PlayerError> {
        Self::start(|events| Engine::spawn(Box::new(sink), events), seed)
    }

    fn start(
        make_engine: impl FnOnce(mpsc::UnboundedSender<Message>) -> Result<Engine, EngineError>,
        seed: u64,
    ) -> Result<Self, PlayerError> {
        let (inbox, receiver) = mpsc::unbounded_channel();
        let engine = make_engine(inbox.clone())?;
        let playback = engine.subscribe();
        let sequence = Sequence::new(seed);
        let (volume, replay_gain) = (Volume::UNITY, ReplayGainMode::default());
        let (publisher, state) =
            watch::channel(Actor::snapshot(&sequence, None, volume, replay_gain));
        let actor = Actor::new(sequence, engine, publisher, volume, replay_gain);
        let thread = thread::Builder::new()
            .name("aeri-player".into())
            .spawn(move || actor.run(receiver))
            .map_err(PlayerError::Spawn)?;
        Ok(Self {
            inner: Arc::new(Inner {
                inbox,
                thread: Some(thread),
            }),
            state,
            playback,
        })
    }

    async fn request<T>(
        &self,
        command: impl FnOnce(Reply<T>) -> Command,
    ) -> Result<T, PlayerError> {
        let (reply, response) = oneshot::channel();
        self.inner
            .inbox
            .send(Message::Command(command(reply)))
            .map_err(|_| PlayerError::Stopped)?;
        response.await.map_err(|_| PlayerError::Stopped)?
    }

    /// Latest-value subscription to everything but playback position. A new subscriber sees
    /// the current state at once.
    #[must_use]
    pub fn subscribe(&self) -> watch::Receiver<PlayerState> {
        self.state.clone()
    }

    /// The engine's own state, passed through: position, transport, output spec.
    #[must_use]
    pub fn playback(&self) -> watch::Receiver<PlaybackState> {
        self.playback.clone()
    }

    /// Creates an empty playlist. The first one created becomes the active playlist.
    ///
    /// # Errors
    ///
    /// [`PlayerError::Stopped`] if the player thread has exited. This applies to every
    /// command below.
    pub async fn create_playlist(&self, name: String) -> Result<PlaylistId, PlayerError> {
        self.request(|reply| Command::CreatePlaylist { name, reply })
            .await
    }

    /// Adds an entry for each track at the end of `to` and returns their ids in order.
    ///
    /// # Errors
    ///
    /// [`PlayerError::Sequence`] if the playlist does not exist.
    pub async fn append(
        &self,
        to: PlaylistId,
        tracks: Vec<Arc<Track>>,
    ) -> Result<Vec<EntryId>, PlayerError> {
        self.request(|reply| Command::Append { to, tracks, reply })
            .await
    }

    /// Removes entries from a playlist, and from the queue. All or none are removed.
    ///
    /// # Errors
    ///
    /// [`PlayerError::Sequence`] if the playlist or any entry does not exist.
    pub async fn remove(&self, from: PlaylistId, entries: Vec<EntryId>) -> Result<(), PlayerError> {
        self.request(|reply| Command::Remove {
            from,
            entries,
            reply,
        })
        .await
    }

    /// Moves `entry` to just before `before`, or to the end when `before` is `None`.
    ///
    /// # Errors
    ///
    /// [`PlayerError::Sequence`] if the playlist, the entry or `before` does not exist.
    pub async fn move_before(
        &self,
        playlist: PlaylistId,
        entry: EntryId,
        before: Option<EntryId>,
    ) -> Result<(), PlayerError> {
        self.request(|reply| Command::MoveBefore {
            playlist,
            entry,
            before,
            reply,
        })
        .await
    }

    /// Makes `playlist` the one the playback order is read from.
    ///
    /// # Errors
    ///
    /// [`PlayerError::Sequence`] if it does not exist.
    pub async fn set_active(&self, playlist: PlaylistId) -> Result<(), PlayerError> {
        self.request(|reply| Command::SetActive { playlist, reply })
            .await
    }

    /// Starts playing, or resumes when paused. Does nothing while playing. When stopped it
    /// starts the entry the order names next, or the first of the active playlist.
    ///
    /// # Errors
    ///
    /// [`PlayerError::NothingToPlay`] when there is no entry to start;
    /// [`PlayerError::Engine`] when the track cannot be played, after the skips of ADR 0025.
    pub async fn play(&self) -> Result<(), PlayerError> {
        self.request(Command::Play).await
    }

    /// Plays this entry now.
    ///
    /// # Errors
    ///
    /// [`PlayerError::Sequence`] if it does not exist; [`PlayerError::Engine`] as for
    /// [`play`](Self::play).
    pub async fn play_entry(
        &self,
        playlist: PlaylistId,
        entry: EntryId,
    ) -> Result<(), PlayerError> {
        self.request(|reply| Command::PlayEntry {
            playlist,
            entry,
            reply,
        })
        .await
    }

    /// Pauses when playing and resumes when paused. The player decides, so two frontends
    /// that both read "playing" cannot undo each other.
    ///
    /// # Errors
    ///
    /// As for [`play`](Self::play), when stopped.
    pub async fn toggle_pause(&self) -> Result<(), PlayerError> {
        self.request(Command::TogglePause).await
    }

    /// Stops playback. The queue and the playlists are kept.
    ///
    /// # Errors
    ///
    /// [`PlayerError::Engine`] if the engine fails to stop.
    pub async fn stop(&self) -> Result<(), PlayerError> {
        self.request(Command::Stop).await
    }

    /// Skips to the next entry, the queue first. Does nothing when there is none.
    ///
    /// # Errors
    ///
    /// As for [`play`](Self::play).
    pub async fn next(&self) -> Result<(), PlayerError> {
        self.request(Command::Next).await
    }

    /// Goes to the entry before the current one, never restarting it. Does nothing when
    /// there is none.
    ///
    /// # Errors
    ///
    /// As for [`play`](Self::play).
    pub async fn previous(&self) -> Result<(), PlayerError> {
        self.request(Command::Previous).await
    }

    /// Moves within the playing track. A backward seek stops at the start.
    ///
    /// # Errors
    ///
    /// [`PlayerError::Engine`] when nothing is playing or the track cannot seek there, such
    /// as past its end; playback carries on as it was.
    pub async fn seek(&self, seek: Seek) -> Result<(), PlayerError> {
        self.request(|reply| Command::Seek { seek, reply }).await
    }

    /// Adds an entry to the back of the queue. It may be queued more than once.
    ///
    /// # Errors
    ///
    /// [`PlayerError::Sequence`] if the playlist or entry does not exist.
    pub async fn enqueue(&self, playlist: PlaylistId, entry: EntryId) -> Result<(), PlayerError> {
        self.request(|reply| Command::Enqueue {
            playlist,
            entry,
            reply,
        })
        .await
    }

    /// Changes how the active playlist is played through.
    ///
    /// # Errors
    ///
    /// [`PlayerError::Stopped`] if the player thread has exited.
    pub async fn set_order(&self, order: PlaybackOrder) -> Result<(), PlayerError> {
        self.request(|reply| Command::SetOrder { order, reply })
            .await
    }

    /// Sets whether playback stops when the current entry ends. The flag clears when it has.
    ///
    /// # Errors
    ///
    /// [`PlayerError::Stopped`] if the player thread has exited.
    pub async fn set_stop_after_current(&self, stop: bool) -> Result<(), PlayerError> {
        self.request(|reply| Command::SetStopAfterCurrent { stop, reply })
            .await
    }

    /// Sets the output volume. It is heard after the audio already buffered.
    ///
    /// # Errors
    ///
    /// [`PlayerError::Engine`] if the engine has exited.
    pub async fn set_volume(&self, volume: Volume) -> Result<(), PlayerError> {
        self.request(|reply| Command::SetVolume { volume, reply })
            .await
    }

    /// Chooses which `ReplayGain` value is applied.
    ///
    /// # Errors
    ///
    /// [`PlayerError::Engine`] if the engine has exited.
    pub async fn set_replay_gain(&self, mode: ReplayGainMode) -> Result<(), PlayerError> {
        self.request(|reply| Command::SetReplayGain { mode, reply })
            .await
    }
}
