//! The ordered list of nodes.

use std::fmt;

use aeri_core::Channels;

use crate::node::{Control, DspNode, TrackStart};

/// The most frames a node is given at once.
pub const BLOCK_FRAMES: usize = 1024;

/// Nodes run in order, each over the output of the one before.
#[derive(Default)]
pub struct Chain {
    nodes: Vec<Box<dyn DspNode>>,
}

impl fmt::Debug for Chain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Chain")
            .field("nodes", &self.nodes.len())
            .finish()
    }
}

impl Chain {
    /// An empty chain, which leaves every block untouched.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `node` after the existing ones.
    pub fn push(&mut self, node: Box<dyn DspNode>) {
        self.nodes.push(node);
    }

    /// Tells every node that a track begins with the next block.
    pub fn start_track(&mut self, start: &TrackStart) {
        for node in &mut self.nodes {
            node.start_track(start);
        }
    }

    /// Sends `control` to every node.
    pub fn control(&mut self, control: &Control) {
        for node in &mut self.nodes {
            node.control(control);
        }
    }

    /// Runs every node over `interleaved`, in pieces of at most [`BLOCK_FRAMES`] frames.
    ///
    /// `channels` must be the channel count of the track the block belongs to. A trailing
    /// partial frame is left unprocessed.
    pub fn process(&mut self, interleaved: &mut [f32], channels: Channels) {
        let width = usize::from(channels.get());
        let whole = interleaved.len() - interleaved.len() % width;
        for piece in interleaved[..whole].chunks_mut(BLOCK_FRAMES * width) {
            for node in &mut self.nodes {
                node.process(piece);
            }
        }
    }

    /// Forgets every node's history after a flush.
    pub fn reset(&mut self) {
        for node in &mut self.nodes {
            node.reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use aeri_core::{ReplayGain, ReplayGainMode, SampleRate, StreamSpec, Volume};
    use proptest::prelude::*;

    use super::*;
    use crate::recorder::{Call, Log, Recorder};

    fn channels(count: u16) -> Channels {
        Channels::new(count).expect("test channel counts are non-zero")
    }

    fn log() -> Log {
        Arc::new(Mutex::new(Vec::new()))
    }

    fn calls(log: &Log) -> Vec<(&'static str, Call)> {
        log.lock().expect("no test thread panicked").clone()
    }

    fn start() -> TrackStart {
        let spec = StreamSpec {
            sample_rate: SampleRate::new(44_100).expect("non-zero"),
            channels: channels(2),
        };
        TrackStart::new(spec, ReplayGain::default())
    }

    #[test]
    fn an_empty_chain_leaves_the_block_untouched() {
        let mut block = [0.25, -0.5, 0.75, 1.0];
        Chain::new().process(&mut block, channels(2));
        assert_eq!(block, [0.25, -0.5, 0.75, 1.0]);
    }

    #[test]
    fn nodes_run_in_the_order_they_were_pushed_and_in_place() {
        let log = log();
        let mut chain = Chain::new();
        chain.push(Recorder::boxed("a", &log, 1.0));
        chain.push(Recorder::boxed("b", &log, 10.0));
        let mut block = [0.0, 0.0];
        chain.process(&mut block, channels(2));
        assert_eq!(block, [11.0, 11.0]);
        assert_eq!(
            calls(&log),
            [
                ("a", Call::Process(vec![0.0, 0.0])),
                ("b", Call::Process(vec![1.0, 1.0]))
            ]
        );
    }

    #[test]
    fn start_track_control_and_reset_reach_every_node_in_order() {
        let log = log();
        let mut chain = Chain::new();
        chain.push(Recorder::boxed("a", &log, 0.0));
        chain.push(Recorder::boxed("b", &log, 0.0));
        chain.start_track(&start());
        chain.control(&Control::Volume(Volume::UNITY));
        chain.control(&Control::ReplayGain(ReplayGainMode::Off));
        chain.reset();
        assert_eq!(
            calls(&log),
            [
                ("a", Call::StartTrack),
                ("b", Call::StartTrack),
                ("a", Call::Control),
                ("b", Call::Control),
                ("a", Call::Control),
                ("b", Call::Control),
                ("a", Call::Reset),
                ("b", Call::Reset),
            ]
        );
    }

    #[test]
    fn a_block_of_exactly_one_piece_is_not_split() {
        let log = log();
        let mut chain = Chain::new();
        chain.push(Recorder::boxed("a", &log, 0.0));
        chain.process(&mut vec![0.0; BLOCK_FRAMES * 2], channels(2));
        assert_eq!(calls(&log).len(), 1);
    }

    #[test]
    fn one_frame_over_the_limit_makes_a_second_piece_of_one_frame() {
        let log = log();
        let mut chain = Chain::new();
        chain.push(Recorder::boxed("a", &log, 0.0));
        chain.process(&mut vec![0.0; (BLOCK_FRAMES + 1) * 2], channels(2));
        assert_eq!(
            calls(&log),
            [
                ("a", Call::Process(vec![0.0; BLOCK_FRAMES * 2])),
                ("a", Call::Process(vec![0.0; 2])),
            ]
        );
    }

    #[test]
    fn a_trailing_partial_frame_is_left_alone() {
        let log = log();
        let mut chain = Chain::new();
        chain.push(Recorder::boxed("a", &log, 1.0));
        let mut block = [0.0, 0.0, 0.0, 0.0, 0.0];
        chain.process(&mut block, channels(2));
        assert_eq!(block, [1.0, 1.0, 1.0, 1.0, 0.0]);
    }

    #[test]
    fn an_empty_block_calls_no_node() {
        let log = log();
        let mut chain = Chain::new();
        chain.push(Recorder::boxed("a", &log, 0.0));
        chain.process(&mut [], channels(2));
        assert_eq!(calls(&log).len(), 0);
    }

    #[test]
    fn debug_shows_the_node_count() {
        let log = log();
        let mut chain = Chain::new();
        chain.push(Recorder::boxed("a", &log, 0.0));
        assert_eq!(format!("{chain:?}"), "Chain { nodes: 1 }");
    }

    proptest! {
        #[test]
        #[cfg_attr(miri, ignore = "proptest reads its regression file, which Miri's isolation forbids")]
        fn any_block_reaches_the_nodes_whole_in_order_and_complete(
            width in 1_u16..=8,
            samples in prop::collection::vec(-1.0_f32..1.0, 0..6000),
        ) {
            let log = log();
            let mut chain = Chain::new();
            chain.push(Recorder::boxed("a", &log, 0.0));
            let mut block = samples.clone();
            chain.process(&mut block, channels(width));

            let width = usize::from(width);
            let mut seen = Vec::new();
            for (_, call) in calls(&log) {
                let Call::Process(piece) = call else { continue };
                prop_assert_eq!(piece.len() % width, 0);
                prop_assert!(!piece.is_empty());
                prop_assert!(piece.len() / width <= BLOCK_FRAMES);
                seen.extend(piece);
            }
            let whole = samples.len() - samples.len() % width;
            prop_assert_eq!(seen, samples[..whole].to_vec());
            prop_assert_eq!(block, samples);
        }
    }
}
