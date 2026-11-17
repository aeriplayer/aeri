//! What the player thread is sent.

use std::sync::Arc;

use aeri_core::{ReplayGainMode, Track, Volume};
use aeri_engine::EngineEvent;
use tokio::sync::oneshot;

use crate::error::PlayerError;
use crate::ids::{EntryId, PlaylistId};
use crate::sequence::PlaybackOrder;
use crate::state::Seek;

/// Where a command's answer goes.
pub(crate) type Reply<T> = oneshot::Sender<Result<T, PlayerError>>;

/// One inbox for commands and engine events, so the thread waits on one receiver.
#[derive(Debug)]
pub(crate) enum Message {
    Command(Command),
    Engine(EngineEvent),
    /// Sent when the last handle is dropped. The engine holds a sender to the inbox too, so
    /// closing the handles' side alone would not close it.
    Shutdown,
}

impl From<EngineEvent> for Message {
    fn from(event: EngineEvent) -> Self {
        Self::Engine(event)
    }
}

#[derive(Debug)]
pub(crate) enum Command {
    CreatePlaylist {
        name: String,
        reply: Reply<PlaylistId>,
    },
    Append {
        to: PlaylistId,
        tracks: Vec<Arc<Track>>,
        reply: Reply<Vec<EntryId>>,
    },
    Remove {
        from: PlaylistId,
        entries: Vec<EntryId>,
        reply: Reply<()>,
    },
    MoveBefore {
        playlist: PlaylistId,
        entry: EntryId,
        before: Option<EntryId>,
        reply: Reply<()>,
    },
    SetActive {
        playlist: PlaylistId,
        reply: Reply<()>,
    },
    Play(Reply<()>),
    PlayEntry {
        playlist: PlaylistId,
        entry: EntryId,
        reply: Reply<()>,
    },
    TogglePause(Reply<()>),
    Stop(Reply<()>),
    Next(Reply<()>),
    Previous(Reply<()>),
    Seek {
        seek: Seek,
        reply: Reply<()>,
    },
    Enqueue {
        playlist: PlaylistId,
        entry: EntryId,
        reply: Reply<()>,
    },
    SetOrder {
        order: PlaybackOrder,
        reply: Reply<()>,
    },
    SetStopAfterCurrent {
        stop: bool,
        reply: Reply<()>,
    },
    SetVolume {
        volume: Volume,
        reply: Reply<()>,
    },
    SetReplayGain {
        mode: ReplayGainMode,
        reply: Reply<()>,
    },
}
