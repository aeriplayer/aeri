//! What the player is doing.

use std::sync::Arc;
use std::time::Duration;

use crate::{StreamSpec, Track};

/// How an active track is moving.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    /// Waiting for decoded audio before output starts.
    Buffering,
    /// Audio is reaching the output.
    Playing,
    /// Output is held; buffered audio is kept.
    Paused,
}

/// What the player is doing.
///
/// A position cannot exist without a track: "stopped at 1:23" is unrepresentable.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlaybackState {
    /// Nothing is loaded.
    Stopped,
    /// A track is loaded, whether playing, paused, or buffering.
    Active {
        /// The track being played.
        track: Arc<Track>,
        /// Whether audio is moving.
        transport: Transport,
        /// How far into `track` the output has played.
        position: Duration,
        /// The spec the output actually negotiated (ADR 0003). `None` until known.
        output: Option<StreamSpec>,
    },
}
