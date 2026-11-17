//! What a frontend reads from the player.

use std::sync::Arc;
use std::time::Duration;

use aeri_core::{ReplayGainMode, Volume};

use crate::ids::{PlaylistId, Position};
use crate::playlist::Playlist;
use crate::sequence::PlaybackOrder;

/// Where to move the playing track to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seek {
    /// To this position from the start.
    To(Duration),
    /// Ahead by this much from where it is now.
    Forward(Duration),
    /// Back by this much from where it is now, to the start at the soonest.
    Back(Duration),
}

/// Everything a frontend renders except playback position, which is on
/// [`Player::playback`](crate::Player::playback).
///
/// Snapshots are shared, not copied: an edit builds a new [`Playlist`] and publishes a new state.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PlayerState {
    /// Every playlist, in the order they were created.
    pub playlists: Arc<[Arc<Playlist>]>,
    /// The playlist the playback order is read from.
    pub active: Option<PlaylistId>,
    /// The entry being heard. It may have been removed from its playlist since.
    pub current: Option<Position>,
    /// The queued entries, the first to play first.
    pub queue: Arc<[Position]>,
    /// How the active playlist is played through.
    pub order: PlaybackOrder,
    /// Whether playback stops when the current entry ends.
    pub stop_after_current: bool,
    /// The output volume.
    pub volume: Volume,
    /// Which `ReplayGain` value is applied.
    pub replay_gain: ReplayGainMode,
}
