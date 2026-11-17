//! The built-in nodes.

use aeri_core::{ReplayGain, ReplayGainMode};

use crate::gain::Gain;
use crate::node::{Control, DspNode, TrackStart};

/// The registry id of the `ReplayGain` node.
pub const REPLAYGAIN: &str = "replaygain";
/// The registry id of the volume node.
pub const VOLUME: &str = "volume";
/// The built-in nodes in the order a player runs them.
///
/// `ReplayGain` comes first, so its peak limit is reached before volume, which only lowers.
pub const DEFAULT_CHAIN: &[&str] = &[REPLAYGAIN, VOLUME];

#[derive(Debug)]
pub(crate) struct ReplayGainNode {
    gain: Gain,
    replay_gain: ReplayGain,
    mode: ReplayGainMode,
    width: usize,
}

impl ReplayGainNode {
    pub(crate) fn boxed() -> Box<dyn DspNode> {
        Box::new(Self {
            gain: Gain::UNITY,
            replay_gain: ReplayGain::default(),
            mode: ReplayGainMode::default(),
            width: 1,
        })
    }

    fn factor(&self) -> f32 {
        self.replay_gain.factor(self.mode)
    }
}

impl DspNode for ReplayGainNode {
    fn start_track(&mut self, start: &TrackStart) {
        self.width = usize::from(start.spec.channels.get());
        self.replay_gain = start.replay_gain;
        self.gain.snap_to(self.factor());
    }

    fn control(&mut self, control: &Control) {
        match control {
            Control::ReplayGain(mode) => {
                self.mode = *mode;
                self.gain.ramp_to(self.factor());
            }
            Control::Volume(_) => {}
        }
    }

    fn process(&mut self, interleaved: &mut [f32]) {
        self.gain.apply(interleaved, self.width);
    }

    fn reset(&mut self) {}
}

#[derive(Debug)]
pub(crate) struct VolumeNode {
    gain: Gain,
    width: usize,
}

impl VolumeNode {
    pub(crate) fn boxed() -> Box<dyn DspNode> {
        Box::new(Self {
            gain: Gain::UNITY,
            width: 1,
        })
    }
}

impl DspNode for VolumeNode {
    fn start_track(&mut self, start: &TrackStart) {
        self.width = usize::from(start.spec.channels.get());
    }

    fn control(&mut self, control: &Control) {
        match control {
            Control::Volume(volume) => self.gain.ramp_to(volume.linear()),
            Control::ReplayGain(_) => {}
        }
    }

    fn process(&mut self, interleaved: &mut [f32]) {
        self.gain.apply(interleaved, self.width);
    }

    fn reset(&mut self) {
        self.gain.settle();
    }
}
