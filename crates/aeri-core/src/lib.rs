//! Playback domain types shared across aeri: the shape of a PCM stream, the track being
//! played, and the state the engine publishes.
//!
//! This crate depends on no other workspace crate. The full metadata model arrives with the
//! library (ADR 0009); playlists and the queue arrive with `aeri-player` (ADR 0005).

mod playback;
mod stream;
mod track;

pub use playback::{PlaybackState, Transport};
pub use stream::{Channels, SampleRate, StreamSpec, ZeroChannelsError, ZeroSampleRateError};
pub use track::{Source, Track};
