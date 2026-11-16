//! The engine against `FakeSink` (brief, Step 5). The tests decide when the fake device
//! plays (`hold`, `allow`, `release`), so no outcome depends on how fast the engine runs.

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;

    use aeri_audio::{FakeSink, FakeSinkObserver, SinkCall};
    use aeri_core::{Channels, PlaybackState, SampleRate, Source, StreamSpec, Track, Transport};
    use aeri_engine::{Engine, EngineError};
    use tokio::sync::watch;
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

    fn engine(sink: FakeSink) -> (Engine, watch::Receiver<PlaybackState>) {
        let engine = Engine::spawn(Box::new(sink)).expect("the playback thread starts");
        let state = engine.subscribe();
        (engine, state)
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

    fn flushes(observer: &FakeSinkObserver) -> usize {
        observer
            .calls()
            .iter()
            .filter(|call| matches!(call, SinkCall::Flush))
            .count()
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore = "decodes files, which Miri's isolation forbids")]
    async fn one_track_buffers_plays_every_frame_and_stops() {
        let (sink, observer) = held_sink();
        let (engine, mut state) = engine(sink);

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
    #[cfg_attr(miri, ignore = "decodes files, which Miri's isolation forbids")]
    async fn pausing_holds_playback_and_resuming_loses_no_frames() {
        let (sink, observer) = held_sink();
        let (engine, mut state) = engine(sink);
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
    #[cfg_attr(miri, ignore = "decodes files, which Miri's isolation forbids")]
    async fn seeking_flushes_once_and_plays_from_near_the_target() {
        let (sink, observer) = held_sink();
        let (engine, mut state) = engine(sink);
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
    #[cfg_attr(miri, ignore = "decodes files, which Miri's isolation forbids")]
    async fn a_next_track_with_the_same_spec_follows_without_reopening_or_stopping() {
        let (sink, observer) = held_sink();
        let (engine, mut state) = engine(sink);
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
    #[cfg_attr(miri, ignore = "decodes files, which Miri's isolation forbids")]
    async fn a_next_track_with_another_spec_reopens_the_output_after_draining() {
        let (sink, observer) = held_sink();
        let (engine, mut state) = engine(sink);
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
    #[cfg_attr(miri, ignore = "decodes files, which Miri's isolation forbids")]
    async fn a_missing_file_is_refused_and_leaves_playback_stopped() {
        let (sink, observer) = held_sink();
        let (engine, state) = engine(sink);

        let result = engine.play(track("missing.flac", "none")).await;

        assert!(matches!(result, Err(EngineError::Decode(_))), "{result:?}");
        assert!(stopped(&state.borrow()));
        assert_eq!(observer.calls(), []);
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore = "decodes files, which Miri's isolation forbids")]
    async fn the_state_reports_the_spec_the_output_negotiated() {
        let sink =
            FakeSink::new(NonZeroUsize::new(CAPACITY).expect("non-zero")).negotiating(spec(48_000));
        sink.observer().hold();
        let (engine, state) = engine(sink);

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
    #[cfg_attr(miri, ignore = "decodes files, which Miri's isolation forbids")]
    async fn dropping_every_handle_stops_and_joins_the_thread() {
        let (sink, _observer) = held_sink();
        let (engine, mut state) = engine(sink);
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
    #[cfg_attr(miri, ignore = "decodes files, which Miri's isolation forbids")]
    async fn clearing_the_next_slot_stops_after_the_current_track() {
        let (sink, observer) = held_sink();
        let (engine, mut state) = engine(sink);
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
    #[cfg_attr(miri, ignore = "decodes files, which Miri's isolation forbids")]
    async fn stopping_discards_buffered_audio_and_plays_no_more() {
        let (sink, observer) = held_sink();
        let (engine, state) = engine(sink);
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
}
