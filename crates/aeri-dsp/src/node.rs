//! The node trait and what a node is told.

use aeri_core::{ReplayGain, ReplayGainMode, StreamSpec, Volume};

/// What a node is told before the first block of each track.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct TrackStart {
    /// The shape of the track's stream.
    pub spec: StreamSpec,
    /// The `ReplayGain` values the track's file carries.
    pub replay_gain: ReplayGain,
}

impl TrackStart {
    /// Describes a track that is about to start.
    #[must_use]
    pub const fn new(spec: StreamSpec, replay_gain: ReplayGain) -> Self {
        Self { spec, replay_gain }
    }
}

/// A setting sent to every node in the chain. A node ignores what is not its own.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum Control {
    /// The output volume.
    Volume(Volume),
    /// Which `ReplayGain` value to apply.
    ReplayGain(ReplayGainMode),
}

/// One stage of the chain. Nodes are compiled in and named by registry id.
///
/// A node never changes the stream spec: it processes in place, so frames in equal frames out.
pub trait DspNode: Send {
    /// A new track begins with the next block. Not on the hot path; may allocate.
    fn start_track(&mut self, start: &TrackStart);

    /// Applies a setting, or ignores it if it is not this node's.
    fn control(&mut self, control: &Control);

    /// Processes whole interleaved frames in place. Allocates nothing.
    fn process(&mut self, interleaved: &mut [f32]);

    /// Forgets history after a flush (seek, stop, skip).
    fn reset(&mut self);
}
