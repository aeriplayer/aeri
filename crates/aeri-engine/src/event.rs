//! What the engine tells its owner (ADR 0023).

use std::sync::Arc;

use aeri_core::Track;

/// Something that happened in the engine and must not be missed (ADR 0004).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum EngineEvent {
    /// The engine took the next slot and is decoding this track. The slot is empty.
    ///
    /// This fires when the current decoder is exhausted, up to a ring's length before the
    /// change is heard, so the owner can refill the slot in time. A later `set_next` fills
    /// the slot for the track after this one.
    TookNext(Arc<Track>),
    /// A track reached by taking the slot is now the one being heard.
    ///
    /// `play` emits none: its reply says the track started.
    NowPlaying(Arc<Track>),
    /// A track could not be opened or failed while decoding, and was skipped.
    ///
    /// A slot track that cannot be opened is reported when it is set, and the slot stays empty:
    /// the current track plays on, and the owner can offer another one.
    Failed(Arc<Track>),
    /// Playback stopped on its own: the current track ended with the slot empty, or the
    /// output failed. An explicit `stop` emits nothing.
    ///
    /// The state already reads stopped when this arrives.
    Ended,
}
