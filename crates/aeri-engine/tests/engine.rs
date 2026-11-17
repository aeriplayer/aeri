//! The engine against `FakeSink` (brief, Step 5). The tests decide when the fake device
//! plays (`hold`, `allow`, `release`), so no outcome depends on how fast the engine runs.

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;

    use aeri_audio::{FakeSink, FakeSinkObserver, SinkCall};
    use aeri_core::{
        Channels, PlaybackState, ReplayGainMode, SampleRate, Source, StreamSpec, Track, Transport,
        Volume,
    };
    use aeri_engine::{Engine, EngineError, EngineEvent};
    use tokio::sync::{mpsc, watch};
    use tokio::time::timeout;

    const FRAMES: usize = 44_100;
    /// Small enough that a held device stops the engine long before a fixture ends.
    const CAPACITY: usize = 4_096;
    const PATIENCE: Duration = Duration::from_secs(5);

    fn spec(hz: u32) -> StreamSpec {
        StreamSpec {
            sample_rate: SampleRate::new(hz).expect("non-zero"),
            channels: Channels::new(2).expect("non-zero"),
        }
    }

    fn track(file: &str, title: &str) -> Arc<Track> {
        let path = PathBuf::from(file);
        Arc::new(Track {
            source: Source::File(path),
            duration: None,
            title: Some(title.to_owned()),
            artist: None,
            album: None,
        })
    }

    fn held_sink() -> (FakeSink, FakeSinkObserver) {
        let sink = FakeSink::new(NonZeroUsize::new(CAPACITY).expect("non-zero"));
        let observer = sink.observer();
        observer.hold();
        (sink, observer)
    }

    type Events = mpsc::UnboundedReceiver<EngineEvent>;

    fn engine(sink: FakeSink) -> (Engine, watch::Receiver<PlaybackState>, Events) {
        let (events, received) = mpsc::unbounded_channel();
        let engine = Engine::spawn_with(Box::new(sink), events, Box::new(aeri_engine::fake_open))
            .expect("the playback thread starts");
        let state = engine.subscribe();
        (engine, state, received)
    }

    async fn until(
        state: &mut watch::Receiver<PlaybackState>,
        what: &str,
        condition: impl FnMut(&PlaybackState) -> bool,
    ) -> PlaybackState {
        let found = match timeout(PATIENCE, state.wait_for(condition)).await {
            Ok(Ok(found)) => Some(found.clone()),
            Ok(Err(_)) => panic!("the engine exited while waiting for {what}"),
            Err(_) => None,
        };
        found.unwrap_or_else(|| {
            panic!(
                "timed out waiting for {what}; state is {:?}",
                *state.borrow()
            )
        })
    }

    fn transport(state: &PlaybackState) -> Option<Transport> {
        match state {
            PlaybackState::Active { transport, .. } => Some(*transport),
            _ => None,
        }
    }

    fn title(state: &PlaybackState) -> Option<&str> {
        match state {
            PlaybackState::Active { track, .. } => track.title.as_deref(),
            _ => None,
        }
    }

    fn position(state: &PlaybackState) -> Option<Duration> {
        match state {
            PlaybackState::Active { position, .. } => Some(*position),
            _ => None,
        }
    }

    fn stopped(state: &PlaybackState) -> bool {
        matches!(state, PlaybackState::Stopped)
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

    fn frames(observer: &FakeSinkObserver) -> u64 {
        observer.played().len() as u64 / 2
    }

    fn flushes(observer: &FakeSinkObserver) -> usize {
        observer
            .calls()
            .iter()
            .filter(|call| matches!(call, SinkCall::Flush))
            .count()
    }

    #[tokio::test]
    async fn one_track_buffers_plays_every_frame_and_stops() {
        let (sink, observer) = held_sink();
        let (engine, mut state, _events) = engine(sink);

        engine
            .play(track("sine.flac", "one"))
            .await
            .expect("the fixture plays");
        until(&mut state, "buffering", |s| {
            transport(s) == Some(Transport::Buffering)
        })
        .await;
        observer.allow(1_000);
        until(&mut state, "playing", |s| {
            transport(s) == Some(Transport::Playing)
        })
        .await;
        observer.allow(30_000);
        until(&mut state, "the position to pass 0.6 s", |s| {
            position(s) >= Some(Duration::from_millis(600))
        })
        .await;
        observer.release();
        until(&mut state, "stopped", stopped).await;

        let played = observer.played();
        assert_eq!(played.len() / 2, FRAMES);
        assert!(
            played.iter().any(|sample| sample.abs() > 0.05),
            "played silence"
        );
    }

    #[tokio::test]
    async fn pausing_holds_playback_and_resuming_loses_no_frames() {
        let (sink, observer) = held_sink();
        let (engine, mut state, _events) = engine(sink);
        engine
            .play(track("sine.flac", "one"))
            .await
            .expect("the fixture plays");
        observer.allow(10_000);
        until(&mut state, "playing", |s| {
            transport(s) == Some(Transport::Playing)
        })
        .await;

        engine.pause().await.expect("pause is answered");
        assert_eq!(transport(&state.borrow()), Some(Transport::Paused));
        let before = observer.played().len();
        observer.release();
        // Proving that nothing happens needs a span of time in which it could have.
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(observer.played().len(), before);

        engine.resume().await.expect("resume is answered");
        until(&mut state, "stopped", stopped).await;
        assert_eq!(observer.played().len() / 2, FRAMES);
    }

    #[tokio::test]
    async fn seeking_flushes_once_and_plays_from_near_the_target() {
        let (sink, observer) = held_sink();
        let (engine, mut state, _events) = engine(sink);
        engine
            .play(track("sine.flac", "one"))
            .await
            .expect("the fixture plays");

        engine
            .seek(Duration::from_millis(500))
            .await
            .expect("the fixture seeks");
        observer.release();
        until(&mut state, "stopped", stopped).await;

        assert_eq!(flushes(&observer), 1);
        let after_seek = observer.played_since_flush().len() / 2;
        // FLAC seeks to a block boundary at or before the target; this fixture's blocks
        // are 4608 frames or fewer.
        assert!(
            after_seek.abs_diff(FRAMES / 2) <= 4_608,
            "played {after_seek} frames after seeking"
        );
    }

    #[tokio::test]
    async fn a_next_track_with_the_same_spec_follows_without_reopening_or_stopping() {
        let (sink, observer) = held_sink();
        let (engine, mut state, _events) = engine(sink);
        engine
            .play(track("sine.flac", "first"))
            .await
            .expect("the fixture plays");
        engine
            .set_next(Some(track("sine.flac", "second")))
            .await
            .expect("set_next is answered");

        observer.allow(FRAMES as u64 - 2_000);
        until(&mut state, "the end of the first track", |s| {
            position(s) >= Some(Duration::from_millis(900))
        })
        .await;
        // Let the engine fill the output past the boundary, so it has moved on to decoding
        // the second track while the first is still audible.
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(title(&state.borrow()), Some("first"));

        observer.allow(3_000);
        let switched = until(&mut state, "the second track playing", |s| {
            title(s) == Some("second") && transport(s) == Some(Transport::Playing)
        })
        .await;
        assert!(
            position(&switched) < Some(Duration::from_millis(100)),
            "the second track started at {switched:?}"
        );
        observer.release();
        until(&mut state, "stopped", stopped).await;

        assert_eq!(configures(&observer), [spec(44_100)]);
        assert_eq!(flushes(&observer), 0);
        assert_eq!(observer.played().len() / 2, 2 * FRAMES);
    }

    #[tokio::test]
    async fn a_next_track_with_another_spec_reopens_the_output_after_draining() {
        let (sink, observer) = held_sink();
        let (engine, mut state, _events) = engine(sink);
        engine
            .play(track("sine.flac", "first"))
            .await
            .expect("the fixture plays");
        engine
            .set_next(Some(track("sine-48k.flac", "second")))
            .await
            .expect("set_next is answered");

        observer.release();
        until(&mut state, "the second track", |s| {
            title(s) == Some("second")
        })
        .await;
        until(&mut state, "stopped", stopped).await;

        assert_eq!(configures(&observer), [spec(44_100), spec(48_000)]);
    }

    #[tokio::test]
    async fn a_missing_file_is_refused_and_leaves_playback_stopped() {
        let (sink, observer) = held_sink();
        let (engine, state, _events) = engine(sink);

        let result = engine.play(track("missing.flac", "none")).await;

        assert!(matches!(result, Err(EngineError::Decode(_))), "{result:?}");
        assert!(stopped(&state.borrow()));
        assert_eq!(observer.calls(), []);
    }

    #[tokio::test]
    async fn the_state_reports_the_spec_the_output_negotiated() {
        let sink =
            FakeSink::new(NonZeroUsize::new(CAPACITY).expect("non-zero")).negotiating(spec(48_000));
        sink.observer().hold();
        let (engine, state, _events) = engine(sink);

        engine
            .play(track("sine.flac", "one"))
            .await
            .expect("the fixture plays");

        let output = match &*state.borrow() {
            PlaybackState::Active { output, .. } => *output,
            _ => None,
        };
        assert_eq!(output, Some(spec(48_000)));
    }

    #[tokio::test]
    async fn dropping_every_handle_stops_and_joins_the_thread() {
        let (sink, _observer) = held_sink();
        let (engine, mut state, _events) = engine(sink);
        engine
            .play(track("sine.flac", "one"))
            .await
            .expect("the fixture plays");
        drop(engine.clone());

        timeout(PATIENCE, tokio::task::spawn_blocking(move || drop(engine)))
            .await
            .expect("the thread joined in time")
            .expect("dropping the engine did not panic");
        assert!(
            state.has_changed().is_err(),
            "the drop returned before the thread exited"
        );

        timeout(PATIENCE, async { while state.changed().await.is_ok() {} })
            .await
            .expect("the thread released the state channel");
        assert!(
            stopped(&state.borrow()),
            "the last published state was not Stopped"
        );
    }

    #[tokio::test]
    async fn clearing_the_next_slot_stops_after_the_current_track() {
        let (sink, observer) = held_sink();
        let (engine, mut state, _events) = engine(sink);
        engine
            .play(track("sine.flac", "first"))
            .await
            .expect("the fixture plays");
        engine
            .set_next(Some(track("sine.flac", "second")))
            .await
            .expect("set_next is answered");
        engine.set_next(None).await.expect("set_next is answered");

        observer.release();
        until(&mut state, "stopped", stopped).await;

        assert_eq!(observer.played().len() / 2, FRAMES);
        assert_eq!(configures(&observer), [spec(44_100)]);
    }

    #[tokio::test]
    async fn stopping_discards_buffered_audio_and_plays_no_more() {
        let (sink, observer) = held_sink();
        let (engine, state, _events) = engine(sink);
        engine
            .play(track("sine.flac", "one"))
            .await
            .expect("the fixture plays");

        engine.stop().await.expect("stop is answered");
        assert!(stopped(&state.borrow()));
        observer.release();
        tokio::time::sleep(Duration::from_millis(50)).await;

        assert_eq!(flushes(&observer), 1);
        assert_eq!(observer.played(), []);
    }

    /// Every sample of a fixture, decoded without the engine.
    fn decoded(file: &str) -> Vec<f32> {
        let mut decoder = aeri_engine::fake_open(&track(file, "").source).expect("fixtures decode");
        let mut samples = Vec::new();
        while let Some(block) = decoder.next_block().expect("fixtures decode cleanly") {
            samples.extend_from_slice(block);
        }
        samples
    }

    fn scaled(samples: &[f32], factor: f32) -> Vec<f32> {
        samples.iter().map(|sample| sample * factor).collect()
    }

    fn bits(samples: &[f32]) -> Vec<u32> {
        samples.iter().map(|sample| sample.to_bits()).collect()
    }

    fn half() -> Volume {
        Volume::from_linear(0.5).expect("in range")
    }

    async fn next_event(events: &mut Events) -> EngineEvent {
        timeout(PATIENCE, events.recv())
            .await
            .expect("an event arrived in time")
            .expect("the engine is still sending")
    }

    fn label(event: &EngineEvent) -> String {
        let name = |track: &Arc<Track>| track.title.clone().unwrap_or_default();
        match event {
            EngineEvent::TookNext(track) => format!("took {}", name(track)),
            EngineEvent::NowPlaying(track) => format!("now {}", name(track)),
            EngineEvent::Failed(track) => format!("failed {}", name(track)),
            EngineEvent::Ended => "ended".to_owned(),
            _ => "unknown".to_owned(),
        }
    }

    /// Events up to and including `Ended`.
    async fn events_until_ended(events: &mut Events) -> Vec<String> {
        let mut seen = Vec::new();
        loop {
            let event = next_event(events).await;
            seen.push(label(&event));
            if matches!(event, EngineEvent::Ended) {
                return seen;
            }
        }
    }

    #[tokio::test]
    async fn taking_the_slot_is_announced_before_the_track_is_heard_then_the_end() {
        let (sink, observer) = held_sink();
        let (engine, _state, mut events) = engine(sink);
        engine
            .play(track("sine.flac", "first"))
            .await
            .expect("the fixture plays");
        engine
            .set_next(Some(track("sine.flac", "second")))
            .await
            .expect("set_next is answered");

        observer.release();

        assert_eq!(
            events_until_ended(&mut events).await,
            ["took second", "now second", "ended"]
        );
    }

    #[tokio::test]
    async fn a_next_track_that_cannot_be_opened_fails_at_once_and_the_current_one_plays_on() {
        let (sink, observer) = held_sink();
        let (engine, _state, mut events) = engine(sink);
        engine
            .play(track("sine.flac", "first"))
            .await
            .expect("the fixture plays");

        engine
            .set_next(Some(track("missing.flac", "gone")))
            .await
            .expect("set_next is answered");

        assert_eq!(label(&next_event(&mut events).await), "failed gone");
        assert_eq!(observer.played().len(), 0, "nothing had played yet");
        observer.release();
        assert_eq!(events_until_ended(&mut events).await, ["ended"]);
        assert_eq!(frames(&observer), FRAMES as u64);
    }

    #[tokio::test]
    async fn a_good_track_offered_after_a_failed_one_follows_gaplessly() {
        let (sink, observer) = held_sink();
        let (engine, _state, mut events) = engine(sink);
        engine
            .play(track("sine.flac", "first"))
            .await
            .expect("the fixture plays");
        engine
            .set_next(Some(track("missing.flac", "gone")))
            .await
            .expect("set_next is answered");
        assert_eq!(label(&next_event(&mut events).await), "failed gone");

        engine
            .set_next(Some(track("sine.flac", "second")))
            .await
            .expect("set_next is answered");
        observer.release();

        assert_eq!(
            events_until_ended(&mut events).await,
            ["took second", "now second", "ended"]
        );
        assert_eq!(flushes(&observer), 0);
        assert_eq!(frames(&observer), 2 * FRAMES as u64);
    }

    #[tokio::test]
    async fn a_failed_offer_leaves_the_slot_empty_even_if_a_good_one_was_there() {
        let (sink, observer) = held_sink();
        let (engine, _state, mut events) = engine(sink);
        engine
            .play(track("sine.flac", "first"))
            .await
            .expect("the fixture plays");
        engine
            .set_next(Some(track("sine.flac", "second")))
            .await
            .expect("set_next is answered");

        engine
            .set_next(Some(track("missing.flac", "gone")))
            .await
            .expect("set_next is answered");
        observer.release();

        assert_eq!(
            events_until_ended(&mut events).await,
            ["failed gone", "ended"]
        );
        assert_eq!(frames(&observer), FRAMES as u64);
    }

    #[tokio::test]
    async fn a_slot_set_after_the_take_is_for_the_track_after_the_taken_one() {
        let (sink, observer) = held_sink();
        let (engine, _state, mut events) = engine(sink);
        engine
            .play(track("sine.flac", "first"))
            .await
            .expect("the fixture plays");
        engine
            .set_next(Some(track("sine.flac", "second")))
            .await
            .expect("set_next is answered");

        observer.allow(FRAMES as u64 - 100);
        assert_eq!(label(&next_event(&mut events).await), "took second");
        engine
            .set_next(Some(track("sine.flac", "third")))
            .await
            .expect("set_next is answered");
        observer.release();

        assert_eq!(
            events_until_ended(&mut events).await,
            ["now second", "took third", "now third", "ended"]
        );
        assert_eq!(observer.played().len() / 2, 3 * FRAMES);
    }

    #[tokio::test]
    async fn a_seek_while_a_reopen_is_pending_keeps_the_taken_track() {
        let (sink, observer) = held_sink();
        let (engine, _state, mut events) = engine(sink);
        engine
            .play(track("sine.flac", "first"))
            .await
            .expect("the fixture plays");
        engine
            .set_next(Some(track("sine-48k.flac", "second")))
            .await
            .expect("set_next is answered");

        observer.allow(FRAMES as u64 - 100);
        assert_eq!(label(&next_event(&mut events).await), "took second");
        engine
            .seek(Duration::from_millis(500))
            .await
            .expect("the fixture seeks");
        observer.release();

        assert_eq!(
            events_until_ended(&mut events).await,
            ["now second", "ended"]
        );
        assert_eq!(configures(&observer), [spec(44_100), spec(48_000)]);
    }

    #[tokio::test]
    async fn play_announces_nothing_and_a_stop_is_silent() {
        let (sink, observer) = held_sink();
        let (engine, _state, mut events) = engine(sink);

        engine
            .play(track("sine.flac", "first"))
            .await
            .expect("the fixture plays");
        engine.stop().await.expect("stop is answered");
        observer.release();
        tokio::time::sleep(Duration::from_millis(50)).await;

        assert!(events.try_recv().is_err());
    }

    #[tokio::test]
    async fn playback_carries_on_when_nobody_is_listening_for_events() {
        let (sink, observer) = held_sink();
        let (engine, mut state, events) = engine(sink);
        drop(events);

        engine
            .play(track("sine.flac", "first"))
            .await
            .expect("the fixture plays");
        observer.release();
        until(&mut state, "stopped", stopped).await;

        assert_eq!(observer.played().len() / 2, FRAMES);
    }

    #[tokio::test]
    async fn half_volume_halves_every_frame_the_sink_receives() {
        let (sink, observer) = held_sink();
        let (engine, mut state, _events) = engine(sink);
        engine.set_volume(half()).await.expect("volume is answered");
        engine
            .play(track("sine.flac", "one"))
            .await
            .expect("the fixture plays");

        observer.release();
        until(&mut state, "stopped", stopped).await;

        assert_eq!(
            bits(&observer.played()),
            bits(&scaled(&decoded("sine.flac"), 0.5))
        );
    }

    #[tokio::test]
    async fn a_volume_change_during_playback_ramps_and_then_holds() {
        let (sink, observer) = held_sink();
        let (engine, mut state, _events) = engine(sink);
        engine
            .play(track("sine.flac", "one"))
            .await
            .expect("the fixture plays");
        observer.allow(1_000);
        until(&mut state, "playing", |s| {
            transport(s) == Some(Transport::Playing)
        })
        .await;

        engine.set_volume(half()).await.expect("volume is answered");
        observer.release();
        until(&mut state, "stopped", stopped).await;

        let expected = decoded("sine.flac");
        let played = observer.played();
        assert_eq!(played.len(), expected.len());
        let head = 2_000;
        let tail = 2 * 20_000;
        assert_eq!(bits(&played[..head]), bits(&expected[..head]));
        let from = played.len() - tail;
        assert_eq!(bits(&played[from..]), bits(&scaled(&expected[from..], 0.5)));
    }

    #[tokio::test]
    async fn unity_volume_with_replaygain_off_delivers_the_decoded_samples_untouched() {
        let (sink, observer) = held_sink();
        let (engine, mut state, _events) = engine(sink);
        engine
            .set_replay_gain(ReplayGainMode::Off)
            .await
            .expect("the mode is answered");
        engine
            .play(track("sine-rg.flac", "tagged"))
            .await
            .expect("the fixture plays");

        observer.release();
        until(&mut state, "stopped", stopped).await;

        assert_eq!(bits(&observer.played()), bits(&decoded("sine-rg.flac")));
    }

    #[tokio::test]
    async fn track_gain_changes_on_the_first_frame_of_the_next_track_with_one_configure() {
        let tagged = aeri_engine::fake_open(&track("sine-rg.flac", "").source)
            .expect("fixtures decode")
            .replay_gain()
            .factor(ReplayGainMode::Track);
        assert!(tagged < 1.0, "the fixture should be turned down");
        let (sink, observer) = held_sink();
        let (engine, mut state, _events) = engine(sink);
        engine
            .set_replay_gain(ReplayGainMode::Track)
            .await
            .expect("the mode is answered");
        engine
            .play(track("sine-rg.flac", "tagged"))
            .await
            .expect("the fixture plays");
        engine
            .set_next(Some(track("sine.flac", "untagged")))
            .await
            .expect("set_next is answered");

        observer.release();
        until(&mut state, "stopped", stopped).await;

        let played = observer.played();
        let boundary = 2 * FRAMES;
        assert_eq!(played.len(), 2 * boundary);
        assert_eq!(
            bits(&played[..boundary]),
            bits(&scaled(&decoded("sine-rg.flac"), tagged))
        );
        assert_eq!(bits(&played[boundary..]), bits(&decoded("sine.flac")));
        assert_eq!(configures(&observer), [spec(44_100)]);
    }

    #[tokio::test]
    async fn a_seek_resets_the_chain_so_a_pending_volume_change_does_not_ramp() {
        let (sink, observer) = held_sink();
        let (engine, mut state, _events) = engine(sink);
        engine
            .play(track("sine.flac", "one"))
            .await
            .expect("the fixture plays");
        engine.set_volume(half()).await.expect("volume is answered");

        engine
            .seek(Duration::from_millis(500))
            .await
            .expect("the fixture seeks");
        observer.release();
        until(&mut state, "stopped", stopped).await;

        let mut decoder = aeri_engine::fake_open(&track("sine.flac", "").source).expect("decodes");
        decoder.seek(Duration::from_millis(500)).expect("seeks");
        let mut expected = Vec::new();
        while let Some(block) = decoder.next_block().expect("decodes cleanly") {
            expected.extend_from_slice(block);
        }
        assert_eq!(
            bits(&observer.played_since_flush()),
            bits(&scaled(&expected, 0.5))
        );
    }

    #[tokio::test]
    async fn now_playing_fires_when_the_new_track_is_heard_and_not_before() {
        let (sink, observer) = held_sink();
        let (engine, _state, mut events) = engine(sink);
        engine
            .play(track("sine.flac", "first"))
            .await
            .expect("the fixture plays");
        engine
            .set_next(Some(track("sine.flac", "second")))
            .await
            .expect("set_next is answered");

        observer.allow(FRAMES as u64 - 1_000);
        assert_eq!(label(&next_event(&mut events).await), "took second");
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            events.try_recv().is_err(),
            "the second track was announced before it was heard"
        );

        observer.allow(2_000);
        assert_eq!(label(&next_event(&mut events).await), "now second");
    }

    /// The default engine opens real files. The decoder itself is tested in `aeri-decode` and
    /// fuzzed; these two check only that the engine reaches it (ADR 0029).
    fn real_fixture(file: &str) -> Arc<Track> {
        let path: PathBuf = [
            env!("CARGO_MANIFEST_DIR"),
            "..",
            "aeri-decode",
            "tests",
            "fixtures",
            file,
        ]
        .iter()
        .collect();
        Arc::new(Track {
            source: Source::File(path),
            duration: None,
            title: Some(file.to_owned()),
            artist: None,
            album: None,
        })
    }

    async fn plays_to_the_end(file: &str) {
        let sink = FakeSink::new(NonZeroUsize::new(CAPACITY).expect("non-zero"));
        let observer = sink.observer();
        let (events, _received) = mpsc::unbounded_channel::<EngineEvent>();
        let engine = Engine::spawn(Box::new(sink), events).expect("the playback thread starts");
        let mut state = engine.subscribe();
        engine
            .play(real_fixture(file))
            .await
            .expect("the fixture plays");
        until(&mut state, "playing", |s| transport(s).is_some()).await;
        until(&mut state, "stopped", stopped).await;
        assert_eq!(observer.played().len() as u64 / 2, FRAMES as u64);
    }

    #[tokio::test]
    async fn the_default_engine_plays_a_real_flac_to_the_end() {
        plays_to_the_end("sine.flac").await;
    }

    #[tokio::test]
    async fn the_default_engine_plays_a_real_mp3_to_the_end() {
        plays_to_the_end("sine.mp3").await;
    }
}
