//! The seam between the engine and an audio output.

use std::error::Error;

use aeri_core::StreamSpec;

/// An audio output that the engine pushes interleaved `f32` blocks into.
///
/// Push-style and block-oriented: a device sink hides its ring buffer and real-time
/// callback behind this trait, so neither appears in the engine (ADR 0003).
pub trait OutputSink: Send {
    /// Opens (or re-opens) the output for `requested` and returns the spec actually
    /// negotiated. The two differ when the device or server declines (ADR 0003).
    ///
    /// Re-opening discards anything buffered and resets [`frames_played`](Self::frames_played).
    ///
    /// # Errors
    ///
    /// [`SinkError::NoDevice`] when there is nothing to open, [`SinkError::FormatRefused`]
    /// when no usable spec could be negotiated, [`SinkError::DeviceLost`] when the device
    /// disappeared while opening.
    fn configure(&mut self, requested: StreamSpec) -> Result<StreamSpec, SinkError>;

    /// Offers interleaved samples. Never blocks. Returns how many whole *frames* were
    /// accepted, which may be zero when the buffer is full.
    ///
    /// A trailing partial frame is never accepted.
    ///
    /// # Errors
    ///
    /// [`SinkError::NotConfigured`] before the first successful
    /// [`configure`](Self::configure), [`SinkError::DeviceLost`] once the device is gone.
    fn write(&mut self, interleaved: &[f32]) -> Result<usize, SinkError>;

    /// Stops or resumes consumption without discarding buffered audio.
    fn set_paused(&mut self, paused: bool);

    /// Discards everything buffered but not yet played (seek, stop, track skip).
    fn flush(&mut self);

    /// Frames handed to the device since the last `configure` or `flush`.
    /// Position reporting derives from this, not from how much was decoded.
    fn frames_played(&self) -> u64;

    /// True once every accepted frame has been played.
    fn is_drained(&self) -> bool;
}

/// Why an [`OutputSink`] could not do what was asked.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SinkError {
    /// There is no output device to open.
    #[error("no audio output device is available")]
    NoDevice,

    /// The output accepted no spec it could play.
    #[error(
        "audio output refused {} Hz with {} channels",
        .requested.sample_rate.get(),
        .requested.channels.get()
    )]
    FormatRefused {
        /// The spec that was asked for.
        requested: StreamSpec,
        /// The platform's own error.
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },

    /// The device went away: unplugged, or the audio server stopped.
    #[error("audio output device was lost")]
    DeviceLost {
        /// The platform's own error.
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },

    /// Samples were written before the output was configured. A bug in the caller.
    #[error("audio output was written to before it was configured")]
    NotConfigured,
}

#[cfg(test)]
mod tests {
    use std::io;

    use aeri_core::{Channels, SampleRate};

    use super::*;

    #[test]
    fn format_refused_names_the_spec_and_keeps_the_platform_error() {
        let requested = StreamSpec {
            sample_rate: SampleRate::new(96_000).expect("96 000 is non-zero"),
            channels: Channels::new(2).expect("2 is non-zero"),
        };
        let error = SinkError::FormatRefused {
            requested,
            source: Box::new(io::Error::other("kAudioDeviceUnsupportedFormatError")),
        };

        assert_eq!(
            error.to_string(),
            "audio output refused 96000 Hz with 2 channels"
        );
        assert_eq!(
            error.source().map(ToString::to_string).as_deref(),
            Some("kAudioDeviceUnsupportedFormatError")
        );
    }
}
