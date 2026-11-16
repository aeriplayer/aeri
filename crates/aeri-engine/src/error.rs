//! What can go wrong in the engine.

use std::io;

use aeri_audio::SinkError;
use aeri_decode::DecodeError;

/// Why an [`Engine`](crate::Engine) command failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EngineError {
    /// The playback thread could not be started.
    #[error("failed to start the playback thread")]
    Spawn(#[source] io::Error),

    /// The playback thread has exited, so no command can reach it.
    #[error("the playback thread has stopped")]
    Stopped,

    /// The command needs a current track and there is none.
    #[error("nothing is playing")]
    NotPlaying,

    /// The track could not be opened, decoded, or sought.
    #[error("cannot decode the track")]
    Decode(#[from] DecodeError),

    /// The audio output could not be opened or used.
    #[error("cannot use the audio output")]
    Output(#[from] SinkError),
}
