//! A node that records what it is told, for the crate's tests.

use std::sync::{Arc, Mutex};

use crate::node::{Control, DspNode, TrackStart};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Call {
    StartTrack,
    Control,
    Process(Vec<f32>),
    Reset,
}

pub(crate) type Log = Arc<Mutex<Vec<(&'static str, Call)>>>;

#[derive(Debug)]
pub(crate) struct Recorder {
    name: &'static str,
    log: Log,
    add: f32,
}

impl Recorder {
    pub(crate) fn boxed(name: &'static str, log: &Log, add: f32) -> Box<dyn DspNode> {
        Box::new(Self {
            name,
            log: Arc::clone(log),
            add,
        })
    }

    fn record(&self, call: Call) {
        self.log
            .lock()
            .expect("a test thread panicked while logging")
            .push((self.name, call));
    }
}

impl DspNode for Recorder {
    fn start_track(&mut self, _: &TrackStart) {
        self.record(Call::StartTrack);
    }

    fn control(&mut self, _: &Control) {
        self.record(Call::Control);
    }

    fn process(&mut self, interleaved: &mut [f32]) {
        self.record(Call::Process(interleaved.to_vec()));
        for sample in interleaved {
            *sample += self.add;
        }
    }

    fn reset(&mut self) {
        self.record(Call::Reset);
    }
}
