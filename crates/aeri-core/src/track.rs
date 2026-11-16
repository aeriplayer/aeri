//! What is played.

use std::path::PathBuf;
use std::time::Duration;

/// Where a track's audio comes from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Source {
    /// A file on the local filesystem.
    File(PathBuf),
}

/// The minimal track (ADR 0009). The full metadata type arrives with the library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    /// Where the audio is read from.
    pub source: Source,
    /// Length of the audio, when the container states it.
    pub duration: Option<Duration>,
    /// The title tag.
    pub title: Option<String>,
    /// The artist tag.
    pub artist: Option<String>,
    /// The album tag.
    pub album: Option<String>,
}
