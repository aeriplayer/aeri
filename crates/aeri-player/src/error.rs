//! What can go wrong in the sequence and in the player.

use crate::ids::{EntryId, PlaylistId};

/// Why an edit to the sequence was refused. Nothing was changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SequenceError {
    /// No playlist has this id, because it never existed.
    #[error("playlist {0} does not exist")]
    UnknownPlaylist(PlaylistId),
    /// The playlist has no entry with this id, because it was removed or never added.
    #[error("entry {0} is not in the playlist")]
    UnknownEntry(EntryId),
}

/// Why a [`Player`](crate::Player) command failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PlayerError {
    /// The player's thread could not be started.
    #[error("failed to start the player thread")]
    Spawn(#[source] std::io::Error),

    /// The player's thread has exited, so no command can reach it.
    #[error("the player has stopped")]
    Stopped,

    /// There is nothing to play: the active playlist is empty, or there is none.
    #[error("there is nothing to play")]
    NothingToPlay,

    /// A playlist or an entry named by the command does not exist.
    #[error("no such playlist or entry")]
    Sequence(#[from] SequenceError),

    /// The playback engine refused the command.
    #[error("playback failed")]
    Engine(#[from] aeri_engine::EngineError),
}
