//! Audio decoding: the [`Decoder`] trait, and [`open`] to get one for a [`Source`].
//!
//! Decoding goes through symphonia (ADR 0007), which stays an implementation detail: no
//! symphonia type appears in this crate's API. The supported formats are FLAC and MP3.

mod error;
mod symphonia_decoder;
mod time;

use std::time::Duration;

use aeri_core::{Source, StreamSpec};

pub use error::DecodeError;

use crate::symphonia_decoder::SymphoniaDecoder;

/// A decoder for one track, producing interleaved `f32` blocks.
pub trait Decoder: Send {
    /// The shape of every block this decoder produces.
    fn spec(&self) -> StreamSpec;

    /// The length of the track, when the container states it.
    fn duration(&self) -> Option<Duration>;

    /// Decodes the next block and lends it out as interleaved `f32`.
    /// `Ok(None)` is a clean end of stream. A returned block is never empty and holds
    /// whole frames. The slice borrows the decoder's own buffer, so steady-state decoding
    /// allocates nothing.
    ///
    /// A corrupt packet is skipped with a warning rather than returned as an error.
    ///
    /// # Errors
    ///
    /// [`DecodeError`] when the stream cannot be read any further.
    fn next_block(&mut self) -> Result<Option<&[f32]>, DecodeError>;

    /// Seeks to the nearest decodable point at or before `to`; returns where it landed.
    ///
    /// # Errors
    ///
    /// [`DecodeError::Seek`] when the stream cannot seek there, [`DecodeError::Io`] when
    /// reading failed during the seek. After an error the position is unspecified; seek
    /// again before decoding.
    fn seek(&mut self, to: Duration) -> Result<Duration, DecodeError>;
}

/// Probes `source` and returns a decoder for it.
///
/// # Errors
///
/// [`DecodeError::Io`] when the source cannot be opened, [`DecodeError::Unsupported`] when it
/// is not FLAC or MP3 or has no playable audio track, [`DecodeError::Corrupt`] when its
/// headers are malformed.
pub fn open(source: &Source) -> Result<Box<dyn Decoder>, DecodeError> {
    match source {
        Source::File(path) => Ok(Box::new(SymphoniaDecoder::open_file(path)?)),
        _ => Err(DecodeError::Unsupported("this kind of source")),
    }
}
