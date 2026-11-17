//! Playback domain types shared across aeri: the shape of a PCM stream, the track being
//! played, the state the engine publishes, and the gain applied on the way out.
//!
//! This crate depends on no other workspace crate. The full metadata model arrives with the
//! library (ADR 0009); playlists and the queue arrive with `aeri-player` (ADR 0005).

mod playback;
mod replay_gain;
mod stream;
mod track;
mod volume;

pub use playback::{PlaybackState, Transport};
pub use replay_gain::{ReplayGain, ReplayGainMode};
pub use stream::{Channels, SampleRate, StreamSpec, ZeroChannelsError, ZeroSampleRateError};
pub use track::{Source, Track};
pub use volume::Volume;
