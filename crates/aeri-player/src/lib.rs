//! Playlists, the queue, and playback order: what plays next.
//!
//! [`Sequence`] holds the playlists, the queue and the order, and answers what plays after
//! what, from plain data (ADR 0024). Nothing here starts a thread or touches audio.

mod error;
mod ids;
mod playlist;
mod sequence;
mod shuffle;

pub use error::SequenceError;
pub use ids::{EntryId, PlaylistId, Position};
pub use playlist::{Entry, Playlist};
pub use sequence::{PlaybackOrder, Sequence};
