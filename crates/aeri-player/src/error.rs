//! What can go wrong naming a playlist or an entry.

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
