//! The engine against decoders that are exactly as long, as wide or as faulty as a test says
//! (ADR 0029): track lengths around the decode block, other channel counts, and a decode that
//! fails part-way. The real fixtures are all one second of stereo, so none of this is reachable
//! with them.

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;

    use aeri_audio::{FakeSink, FakeSinkObserver, SinkCall};
    use aeri_core::{Channels, ReplayGain, SampleRate, Source, StreamSpec, Track};
    use aeri_decode::{DecodeError, Decoder, FakeDecoder};
    use aeri_engine::{Engine, EngineEvent};
    use proptest::prelude::*;
    use tokio::sync::mpsc;
    use tokio::time::timeout;

    const PATIENCE: Duration = Duration::from_secs(5);
    const BLOCK: u64 = 4_096;

    /// A track the fake decoder is asked for by index: how long, how wide, and where it fails.
    #[derive(Debug, Clone, Copy)]
    struct Plan {
        frames: u64,
        channels: u16,
        fails_after: Option<u64>,
    }

    impl Plan {
        const fn stereo(frames: u64) -> Self {
            Self {
                frames,
                channels: 2,
                fails_after: None,
            }
        }

        fn decoder(self) -> FakeDecoder {
            let spec = StreamSpec {
                sample_rate: SampleRate::new(44_100).expect("non-zero"),
                channels: Channels::new(self.channels).expect("non-zero"),
            };
            let decoder = FakeDecoder::new(spec, self.frames, ReplayGain::default());
            match self.fails_after {
                Some(frames) => decoder.failing_after(frames),
                None => decoder,
            }
        }
    }

    /// An engine whose tracks are `plans`, named by their index, playing into a sink that never holds.
    fn engine(
        plans: Vec<Plan>,
    ) -> (
        Engine,
        FakeSinkObserver,
        mpsc::UnboundedReceiver<EngineEvent>,
    ) {
        let sink = FakeSink::new(NonZeroUsize::new(4_096).expect("non-zero"));
        let observer = sink.observer();
        observer.release();
        let (events, received) = mpsc::unbounded_channel();
        let open = move |source: &Source| -> Result<Box<dyn Decoder>, DecodeError> {
            let Source::File(path) = source else {
                return Err(DecodeError::Unsupported("this kind of source"));
            };
            let index: usize = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.parse().ok())
                .ok_or(DecodeError::Unsupported("not a planned track"))?;
            plans
                .get(index)
                .map(|plan| Box::new(plan.decoder()) as Box<dyn Decoder>)
                .ok_or(DecodeError::Unsupported("not a planned track"))
        };
        let engine = Engine::spawn_with(Box::new(sink), events, Box::new(open))
            .expect("the playback thread starts");
        (engine, observer, received)
    }

    fn track(index: usize) -> Arc<Track> {
        Arc::new(Track {
            source: Source::File(PathBuf::from(index.to_string())),
            duration: None,
            title: Some(index.to_string()),
            artist: None,
            album: None,
        })
    }

    fn index_of(track: &Track) -> usize {
        track
            .title
            .as_deref()
            .and_then(|title| title.parse().ok())
            .expect("a planned track's title is its index")
    }

    /// What the owner saw, in order.
    #[derive(Debug, Default, PartialEq, Eq)]
    struct Heard {
        took: Vec<usize>,
        now_playing: Vec<usize>,
        failed: Vec<usize>,
        ended: usize,
    }

    /// Plays track 0, feeds the slot with each next track as the engine takes the last one, and
    /// returns what happened once the engine stops on its own.
    async fn play_all(
        engine: &Engine,
        received: &mut mpsc::UnboundedReceiver<EngineEvent>,
        count: usize,
    ) -> Heard {
        engine.play(track(0)).await.expect("the first track plays");
        if count > 1 {
            engine
                .set_next(Some(track(1)))
                .await
                .expect("the slot is set");
        }
        let mut heard = Heard::default();
        loop {
            let event = timeout(PATIENCE, received.recv())
                .await
                .expect("the engine reports an event in time")
                .expect("the engine is running");
            match event {
                EngineEvent::TookNext(taken) => {
                    let taken = index_of(&taken);
                    heard.took.push(taken);
                    if taken + 1 < count {
                        engine
                            .set_next(Some(track(taken + 1)))
                            .await
                            .expect("the slot is set");
                    }
                }
                EngineEvent::NowPlaying(now) => heard.now_playing.push(index_of(&now)),
                EngineEvent::Failed(failed) => heard.failed.push(index_of(&failed)),
                EngineEvent::Ended => {
                    heard.ended += 1;
                    return heard;
                }
                other => panic!("an event this test does not know: {other:?}"),
            }
        }
    }

    fn configures(observer: &FakeSinkObserver) -> Vec<StreamSpec> {
        observer
            .calls()
            .into_iter()
            .filter_map(|call| match call {
                SinkCall::Configure { requested, .. } => Some(requested),
                _ => None,
            })
            .collect()
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("the runtime builds")
    }

    /// Frames the sink holds. A track longer than this cannot finish while the device is held.
    const CAPACITY: u64 = 4_096;

    /// Plays `first` then `second` with the device held until the slot is set, so the engine
    /// cannot reach the end of `first` before `second` is offered, however short `second` is.
    async fn play_pair(
        engine: &Engine,
        observer: &FakeSinkObserver,
        received: &mut mpsc::UnboundedReceiver<EngineEvent>,
    ) -> Heard {
        observer.hold();
        engine.play(track(0)).await.expect("the first track plays");
        engine
            .set_next(Some(track(1)))
            .await
            .expect("the slot is set");
        observer.release();
        let mut heard = Heard::default();
        loop {
            let event = timeout(PATIENCE, received.recv())
                .await
                .expect("the engine reports an event in time")
                .expect("the engine is running");
            match event {
                EngineEvent::TookNext(taken) => heard.took.push(index_of(&taken)),
                EngineEvent::NowPlaying(now) => heard.now_playing.push(index_of(&now)),
                EngineEvent::Failed(failed) => heard.failed.push(index_of(&failed)),
                EngineEvent::Ended => {
                    heard.ended += 1;
                    return heard;
                }
                other => panic!("an event this test does not know: {other:?}"),
            }
        }
    }

    /// Lengths of the second track: anything, including either side of a decode block.
    fn any_length() -> impl Strategy<Value = u64> {
        prop_oneof![
            0_u64..=9_000,
            Just(0),
            Just(1),
            Just(BLOCK - 1),
            Just(BLOCK),
            Just(BLOCK + 1),
            Just(2 * BLOCK),
        ]
    }

    /// Lengths of the first track: long enough that it cannot end while the device is held.
    fn first_length() -> impl Strategy<Value = u64> {
        prop_oneof![
            (CAPACITY + 1)..=9_000,
            Just(CAPACITY + 1),
            Just(BLOCK + 1),
            Just(2 * BLOCK),
            Just(2 * BLOCK + 1),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(48))]

        #[test]
        fn a_second_track_of_any_length_follows_the_first_with_every_frame_and_no_gap(
            first in first_length(),
            second in any_length(),
        ) {
            let (engine, observer, mut received) =
                engine(vec![Plan::stereo(first), Plan::stereo(second)]);

            let heard = runtime().block_on(play_pair(&engine, &observer, &mut received));

            prop_assert_eq!(observer.played().len() as u64, 2 * (first + second));
            prop_assert_eq!(heard.took, vec![1]);
            prop_assert_eq!(heard.now_playing, vec![1]);
            prop_assert_eq!(heard.failed, Vec::<usize>::new());
            prop_assert_eq!(heard.ended, 1);
            prop_assert_eq!(configures(&observer).len(), 1, "one output for one spec");
        }
    }

    #[test]
    fn a_next_track_with_no_audio_adds_nothing_and_the_engine_then_stops() {
        let (engine, observer, mut received) = engine(vec![Plan::stereo(5_000), Plan::stereo(0)]);

        let heard = runtime().block_on(play_pair(&engine, &observer, &mut received));

        assert_eq!(observer.played().len(), 2 * 5_000);
        assert_eq!(heard.took, [1]);
        assert_eq!(heard.failed, Vec::<usize>::new());
        assert_eq!(heard.ended, 1);
    }

    #[test]
    fn a_decode_that_fails_part_way_loses_the_rest_of_that_track_and_the_next_follows() {
        let failing = Plan {
            fails_after: Some(6_000),
            ..Plan::stereo(20_000)
        };
        let (engine, observer, mut received) = engine(vec![failing, Plan::stereo(3_000)]);

        let heard = runtime().block_on(play_all(&engine, &mut received, 2));

        assert_eq!(heard.failed, [0], "the failing track is reported once");
        assert_eq!(heard.took, [1]);
        assert_eq!(
            observer.played().len(),
            2 * (6_000 + 3_000),
            "what decoded before the fault played, then the next track, with no gap"
        );
        assert_eq!(heard.ended, 1);
    }

    #[test]
    fn a_decode_that_fails_on_the_only_track_reports_it_and_stops() {
        let failing = Plan {
            fails_after: Some(100),
            ..Plan::stereo(1_000)
        };
        let (engine, observer, mut received) = engine(vec![failing]);

        let heard = runtime().block_on(play_all(&engine, &mut received, 1));

        assert_eq!(heard.failed, [0]);
        assert_eq!(observer.played().len(), 2 * 100);
        assert_eq!(heard.ended, 1);
    }

    #[test]
    fn mono_stereo_and_surround_tracks_each_play_every_frame_in_their_own_width() {
        for channels in [1_u16, 2, 3, 6, 8] {
            let (engine, observer, mut received) = engine(vec![Plan {
                channels,
                ..Plan::stereo(10_000)
            }]);

            let heard = runtime().block_on(play_all(&engine, &mut received, 1));

            assert_eq!(
                observer.played().len() as u64,
                10_000 * u64::from(channels),
                "{channels} channels"
            );
            assert_eq!(
                configures(&observer)
                    .iter()
                    .map(|spec| spec.channels.get())
                    .collect::<Vec<_>>(),
                [channels]
            );
            assert_eq!(heard.ended, 1);
        }
    }

    #[test]
    fn a_change_of_width_between_tracks_reopens_the_output_and_loses_no_frame() {
        let (engine, observer, mut received) = engine(vec![
            Plan::stereo(5_000),
            Plan {
                channels: 1,
                ..Plan::stereo(7_000)
            },
        ]);

        let heard = runtime().block_on(play_all(&engine, &mut received, 2));

        assert_eq!(observer.played().len() as u64, 5_000 * 2 + 7_000);
        assert_eq!(
            configures(&observer)
                .iter()
                .map(|spec| spec.channels.get())
                .collect::<Vec<_>>(),
            [2, 1]
        );
        assert_eq!(heard.took, [1]);
    }
}
