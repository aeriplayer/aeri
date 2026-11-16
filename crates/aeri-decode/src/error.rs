//! What can go wrong while decoding.

use std::io;

/// Why a track could not be opened, decoded, or sought.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DecodeError {
    /// Reading the source failed: missing file, permissions, a failing disk.
    #[error("failed to read audio")]
    Io(#[from] io::Error),

    /// The source is not a supported format, or uses a feature of it that is not supported.
    #[error("unsupported audio: {0}")]
    Unsupported(&'static str),

    /// The source claims a supported format but its structure is malformed.
    #[error("corrupt audio: {0}")]
    Corrupt(&'static str),

    /// The stream cannot seek to the requested position.
    #[error("cannot seek: {0}")]
    Seek(&'static str),
}
