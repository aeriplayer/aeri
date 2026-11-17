//! One benchmark per built-in node and one for the default chain, over a 1024-frame stereo block.

use aeri_core::{Channels, ReplayGain, SampleRate, StreamSpec, Volume};
use aeri_dsp::{
    BLOCK_FRAMES, Chain, Control, DEFAULT_CHAIN, REPLAYGAIN, Registry, TrackStart, VOLUME,
};
use divan::{Bencher, black_box};

const CHANNELS: u16 = 2;

fn main() {
    divan::main();
}

fn channels() -> Channels {
    Channels::new(CHANNELS).expect("the channel count is a non-zero constant")
}

fn settled_chain(ids: &[&str]) -> Chain {
    let mut chain = Registry::builtin()
        .build(ids)
        .expect("the ids are built in");
    let spec = StreamSpec {
        sample_rate: SampleRate::new(44_100).expect("non-zero"),
        channels: channels(),
    };
    let replay_gain = ReplayGain {
        track_gain_db: Some(-6.0),
        ..ReplayGain::default()
    };
    chain.start_track(&TrackStart::new(spec, replay_gain));
    chain.control(&Control::Volume(Volume::from_db(-6.0)));
    let mut warm_up = block();
    chain.process(&mut warm_up, channels());
    chain
}

fn block() -> Vec<f32> {
    vec![0.5; BLOCK_FRAMES * usize::from(CHANNELS)]
}

fn bench_chain(bencher: Bencher<'_, '_>, ids: &[&str]) {
    let mut chain = settled_chain(ids);
    let mut samples = block();
    bencher.bench_local(|| chain.process(black_box(&mut samples), channels()));
}

#[divan::bench]
fn replaygain(bencher: Bencher<'_, '_>) {
    bench_chain(bencher, &[REPLAYGAIN]);
}

#[divan::bench]
fn volume(bencher: Bencher<'_, '_>) {
    bench_chain(bencher, &[VOLUME]);
}

#[divan::bench]
fn default_chain(bencher: Bencher<'_, '_>) {
    bench_chain(bencher, DEFAULT_CHAIN);
}
