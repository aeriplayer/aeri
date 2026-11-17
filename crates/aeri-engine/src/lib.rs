//! The playback engine: an actor that decodes a current track and a next slot into an output.
//!
//! One named thread owns all playback state (ADR 0004). [`Engine`] is a cheap handle to it:
//! commands go in on a channel and are answered one by one; state comes out on a watch
//! channel. The engine knows two tracks, the one playing and an optional next, and nothing
//! about playlists (ADR 0005). What happens to the owner's tracks is reported as
//! [`EngineEvent`]s on a channel the owner provides (ADR 0023).
//!
//! The `test-util` feature re-exports the fake output sink, for tests of code that drives
//! the real engine. It is for dev-dependencies only.

mod error;
mod event;
mod playback;

use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use aeri_audio::OutputSink;
use aeri_core::{PlaybackState, ReplayGainMode, Source, Track, Volume};
use aeri_decode::{DecodeError, Decoder};
use tokio::sync::{mpsc, oneshot, watch};

#[cfg(feature = "test-util")]
pub use aeri_audio::{FakeSink, FakeSinkObserver, SinkCall};
pub use aeri_decode::EXTENSIONS;
#[cfg(feature = "test-util")]
pub use aeri_decode::{FakeDecoder, fake_open};

pub use crate::error::EngineError;
pub use crate::event::EngineEvent;
use crate::playback::{Command, Playback};

/// How a track is opened: [`aeri_decode::open`] in the engine you get from
/// [`spawn`](Engine::spawn), anything else through `spawn_with` (ADR 0029).
pub type Opener = Box<dyn Fn(&Source) -> Result<Box<dyn Decoder>, DecodeError> + Send>;

/// Commands may queue this deep before senders wait.
const COMMAND_QUEUE: usize = 32;

/// A cheap, cloneable handle to the playback thread.
///
/// When the last handle is dropped, the thread stops its output and exits, and the drop
/// waits for it. There is no shutdown method to forget.
#[derive(Debug, Clone)]
pub struct Engine {
    inner: Arc<Inner>,
    state: watch::Receiver<PlaybackState>,
}

#[derive(Debug)]
struct Inner {
    commands: Option<mpsc::Sender<Command>>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        // Closing the channel is what tells the thread to exit, so it must happen first.
        drop(self.commands.take());
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::error!("the playback thread panicked");
        }
    }
}

impl Engine {
    /// Starts the playback thread, which takes ownership of `sink`.
    ///
    /// Events go to `events`, converted with `E::from`, so an owner can pass the sender of
    /// its own inbox. The channel is unbounded, so the playback thread never waits on a
    /// slow receiver. A dropped receiver is ignored.
    ///
    /// # Errors
    ///
    /// [`EngineError::Spawn`] if the operating system refuses a new thread.
    pub fn spawn<E>(
        sink: Box<dyn OutputSink>,
        events: mpsc::UnboundedSender<E>,
    ) -> Result<Self, EngineError>
    where
        E: From<EngineEvent> + Send + 'static,
    {
        Self::start(sink, events, Box::new(aeri_decode::open))
    }

    /// As [`spawn`](Self::spawn), opening tracks with `open`: for tests that need no audio files.
    ///
    /// # Errors
    ///
    /// As for [`spawn`](Self::spawn).
    #[cfg(feature = "test-util")]
    pub fn spawn_with<E>(
        sink: Box<dyn OutputSink>,
        events: mpsc::UnboundedSender<E>,
        open: Opener,
    ) -> Result<Self, EngineError>
    where
        E: From<EngineEvent> + Send + 'static,
    {
        Self::start(sink, events, open)
    }

    fn start<E>(
        sink: Box<dyn OutputSink>,
        events: mpsc::UnboundedSender<E>,
        open: Opener,
    ) -> Result<Self, EngineError>
    where
        E: From<EngineEvent> + Send + 'static,
    {
        let (commands, receiver) = mpsc::channel(COMMAND_QUEUE);
        let (publisher, state) = watch::channel(PlaybackState::Stopped);
        let emit = Box::new(move |event: EngineEvent| {
            if events.send(E::from(event)).is_err() {
                tracing::debug!("nothing is listening for engine events");
            }
        });
        let thread = thread::Builder::new()
            .name("aeri-playback".into())
            .spawn(move || Playback::new(sink, receiver, publisher, emit, open).run())
            .map_err(EngineError::Spawn)?;
        Ok(Self {
            inner: Arc::new(Inner {
                commands: Some(commands),
                thread: Some(thread),
            }),
            state,
        })
    }

    /// Starts the playback thread on this platform's default output device.
    ///
    /// # Errors
    ///
    /// [`EngineError::Spawn`] as for [`spawn`](Self::spawn). On platforms without a device
    /// sink, [`EngineError::Output`].
    pub fn with_default_output<E>(events: mpsc::UnboundedSender<E>) -> Result<Self, EngineError>
    where
        E: From<EngineEvent> + Send + 'static,
    {
        #[cfg(target_os = "macos")]
        {
            Self::spawn(Box::new(aeri_audio::CoreAudioSink::new()), events)
        }
        #[cfg(target_os = "linux")]
        {
            Self::spawn(Box::new(aeri_audio::PipeWireSink::new()), events)
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            drop(events);
            Err(EngineError::Output(aeri_audio::SinkError::NoDevice))
        }
    }

    /// Plays `track` now, replacing the current track and clearing the next slot.
    ///
    /// # Errors
    ///
    /// [`EngineError::Decode`] if the track cannot be opened, leaving playback as it was;
    /// [`EngineError::Output`] if the output cannot be opened for it, which stops playback.
    pub async fn play(&self, track: Arc<Track>) -> Result<(), EngineError> {
        self.request(|reply| Command::Play { track, reply }).await
    }

    /// Fills or clears the slot the engine takes when the current track ends.
    ///
    /// # Errors
    ///
    /// [`EngineError::Stopped`] if the playback thread has exited.
    pub async fn set_next(&self, track: Option<Arc<Track>>) -> Result<(), EngineError> {
        self.request(|reply| Command::SetNext { track, reply })
            .await
    }

    /// Holds playback where it is. Does nothing when stopped or already paused.
    ///
    /// # Errors
    ///
    /// [`EngineError::Stopped`] if the playback thread has exited.
    pub async fn pause(&self) -> Result<(), EngineError> {
        self.request(Command::Pause).await
    }

    /// Continues after [`pause`](Self::pause). Does nothing unless paused.
    ///
    /// # Errors
    ///
    /// [`EngineError::Stopped`] if the playback thread has exited.
    pub async fn resume(&self) -> Result<(), EngineError> {
        self.request(Command::Resume).await
    }

    /// Stops playback and discards buffered audio. The next slot is kept.
    ///
    /// # Errors
    ///
    /// [`EngineError::Stopped`] if the playback thread has exited.
    pub async fn stop(&self) -> Result<(), EngineError> {
        self.request(Command::Stop).await
    }

    /// Moves the current track to the nearest decodable point at or before `to`.
    ///
    /// # Errors
    ///
    /// [`EngineError::NotPlaying`] when stopped; [`EngineError::Decode`] when the track
    /// cannot seek there, in which case it carries on from wherever the decoder is.
    pub async fn seek(&self, to: Duration) -> Result<(), EngineError> {
        self.request(|reply| Command::Seek { to, reply }).await
    }

    /// Sets the output volume. The change ramps in over the next block and is heard after
    /// the audio already buffered, about a quarter of a second (ADR 0016).
    ///
    /// # Errors
    ///
    /// [`EngineError::Stopped`] if the playback thread has exited.
    pub async fn set_volume(&self, volume: Volume) -> Result<(), EngineError> {
        self.request(|reply| Command::SetVolume { volume, reply })
            .await
    }

    /// Chooses which `ReplayGain` value is applied, from the next track on and at once for
    /// the current one.
    ///
    /// # Errors
    ///
    /// [`EngineError::Stopped`] if the playback thread has exited.
    pub async fn set_replay_gain(&self, mode: ReplayGainMode) -> Result<(), EngineError> {
        self.request(|reply| Command::SetReplayGain { mode, reply })
            .await
    }

    /// Latest-value subscription. A new subscriber sees the current state at once.
    #[must_use]
    pub fn subscribe(&self) -> watch::Receiver<PlaybackState> {
        self.state.clone()
    }

    async fn request(
        &self,
        command: impl FnOnce(oneshot::Sender<Result<(), EngineError>>) -> Command,
    ) -> Result<(), EngineError> {
        let commands = self.inner.commands.as_ref().ok_or(EngineError::Stopped)?;
        let (reply, response) = oneshot::channel();
        commands
            .send(command(reply))
            .await
            .map_err(|_| EngineError::Stopped)?;
        response.await.map_err(|_| EngineError::Stopped)?
    }
}
