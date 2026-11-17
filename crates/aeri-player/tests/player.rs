//! The player against the real engine over a fake sink (brief, Step 7). The tests decide
//! when the fake device plays (`hold`, `allow`, `release`), so no outcome depends on how fast
//! the threads run.

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;

    use aeri_core::{PlaybackState, ReplayGainMode, Source, Track, Transport, Volume};
    use aeri_engine::{FakeSink, FakeSinkObserver, SinkCall};
    use aeri_player::{
        EntryId, PlaybackOrder, Player, PlayerError, PlayerState, PlaylistId, Position, Seek,
        Sequence,
    };
    use tokio::sync::watch;
    use tokio::time::timeout;

    const FRAMES: u64 = 44_100;
    const CAPACITY: usize = 4_096;
    const PATIENCE: Duration = Duration::from_secs(10);
    const SEED: u64 = 42;

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

    fn good(title: &str) -> Arc<Track> {
        track("sine.flac", title)
    }

    fn missing(title: &str) -> Arc<Track> {
        track("missing.flac", title)
    }

    struct Rig {
        player: Player,
        observer: FakeSinkObserver,
        state: watch::Receiver<PlayerState>,
        playback: watch::Receiver<PlaybackState>,
        list: PlaylistId,
        ids: Vec<EntryId>,
    }

    impl Rig {
        fn at(&self, index: usize) -> Position {
            Position {
                playlist: self.list,
                entry: self.ids[index],
            }
        }

        async fn current_is(&mut self, index: usize) -> PlayerState {
            let wanted = Some(self.at(index));
            until_state(&mut self.state, &format!("entry {index} to play"), |s| {
                s.current == wanted
            })
            .await
        }

        async fn stopped(&mut self) {
            until_state(&mut self.state, "no current entry", |s| s.current.is_none()).await;
            until_playback(&mut self.playback, "stopped", |s| {
                matches!(s, PlaybackState::Stopped)
            })
            .await;
        }
    }

    async fn rig(tracks: Vec<Arc<Track>>) -> Rig {
        let sink = FakeSink::new(NonZeroUsize::new(CAPACITY).expect("non-zero"));
        let observer = sink.observer();
        observer.hold();
        let player = Player::with_fake_sink(sink, SEED).expect("the threads start");
        let state = player.subscribe();
        let playback = player.playback();
        let list = player
            .create_playlist("list".to_owned())
            .await
            .expect("the player is running");
        let ids = player
            .append(list, tracks)
            .await
            .expect("the playlist exists");
        Rig {
            player,
            observer,
            state,
            playback,
            list,
            ids,
        }
    }

    async fn until_state(
        state: &mut watch::Receiver<PlayerState>,
        what: &str,
        condition: impl FnMut(&PlayerState) -> bool,
    ) -> PlayerState {
        let found = match timeout(PATIENCE, state.wait_for(condition)).await {
            Ok(Ok(found)) => Some(found.clone()),
            Ok(Err(_)) => panic!("the player exited while waiting for {what}"),
            Err(_) => None,
        };
        found.unwrap_or_else(|| {
            panic!(
                "timed out waiting for {what}; current is {:?}",
                state.borrow().current
            )
        })
    }

    async fn until_playback(
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
                "timed out waiting for {what}; playback is {:?}",
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

    fn position(state: &PlaybackState) -> Option<Duration> {
        match state {
            PlaybackState::Active { position, .. } => Some(*position),
            _ => None,
        }
    }

    fn configures(observer: &FakeSinkObserver) -> usize {
        observer
            .calls()
            .iter()
            .filter(|call| matches!(call, SinkCall::Configure { .. }))
            .count()
    }

    /// A restart (`engine.play`) flushes the output; a gapless change does not.
    fn flushes(observer: &FakeSinkObserver) -> usize {
        observer
            .calls()
            .iter()
            .filter(|call| matches!(call, SinkCall::Flush))
            .count()
    }

    fn frames(observer: &FakeSinkObserver) -> u64 {
        observer.played().len() as u64 / 2
    }

    // ---- 1: gapless across a playlist ----

    #[tokio::test]
    async fn three_same_spec_tracks_play_gaplessly_in_order_then_stop() {
        let mut rig = rig(vec![good("a"), good("b"), good("c")]).await;

        rig.player.play().await.expect("the fixture plays");
        rig.current_is(0).await;
        rig.observer.allow(FRAMES + 500);
        rig.current_is(1).await;
        rig.observer.allow(FRAMES);
        rig.current_is(2).await;
        rig.observer.release();
        rig.stopped().await;

        assert_eq!(configures(&rig.observer), 1);
        assert_eq!(
            flushes(&rig.observer),
            0,
            "a track restarted instead of following"
        );
        assert_eq!(frames(&rig.observer), 3 * FRAMES);
    }

    // ---- 2: the orders ----

    #[tokio::test]
    async fn repeat_track_plays_the_same_entry_again_and_again() {
        let mut rig = rig(vec![good("a"), good("b")]).await;
        rig.player
            .set_order(PlaybackOrder::RepeatTrack)
            .await
            .expect("the player is running");

        rig.player.play().await.expect("the fixture plays");
        rig.observer.allow(2 * FRAMES + 1_000);
        until_playback(&mut rig.playback, "the third pass", |s| {
            position(s) >= Some(Duration::ZERO) && matches!(s, PlaybackState::Active { .. })
        })
        .await;
        tokio::time::sleep(Duration::from_millis(100)).await;

        assert_eq!(rig.state.borrow().current, Some(rig.at(0)));
        assert_eq!(frames(&rig.observer), 2 * FRAMES + 1_000);
        rig.player.stop().await.expect("stop is answered");
    }

    #[tokio::test]
    async fn repeat_playlist_goes_back_to_the_first_entry_after_the_last() {
        let mut rig = rig(vec![good("a"), good("b")]).await;
        rig.player
            .set_order(PlaybackOrder::RepeatPlaylist)
            .await
            .expect("the player is running");

        rig.player.play().await.expect("the fixture plays");
        rig.observer.allow(FRAMES + 500);
        rig.current_is(1).await;
        rig.observer.allow(FRAMES);
        rig.current_is(0).await;

        rig.player.stop().await.expect("stop is answered");
    }

    #[tokio::test]
    async fn shuffle_plays_the_order_a_sequence_with_the_same_seed_gives() {
        let mut model = Sequence::new(SEED);
        let list = model.create_playlist("list".to_owned());
        let ids = model
            .append(list, vec![good("a"), good("b"), good("c"), good("d")])
            .expect("exists");
        model.set_order(PlaybackOrder::Shuffle);
        let expected: Vec<EntryId> = (0..4)
            .map(|_| model.advance().expect("shuffle never ends").entry)
            .collect();

        let mut rig = rig(vec![good("a"), good("b"), good("c"), good("d")]).await;
        assert_eq!(rig.ids, ids);
        rig.player
            .set_order(PlaybackOrder::Shuffle)
            .await
            .expect("the player is running");
        rig.player.play().await.expect("the fixture plays");

        for (index, entry) in expected.iter().enumerate() {
            let wanted = Some(Position {
                playlist: rig.list,
                entry: *entry,
            });
            until_state(&mut rig.state, "the next shuffled entry", |s| {
                s.current == wanted
            })
            .await;
            rig.observer
                .allow(FRAMES + if index == 0 { 500 } else { 0 });
        }
        rig.player.stop().await.expect("stop is answered");
    }

    // ---- 3: the queue ----

    #[tokio::test]
    async fn a_queued_entry_plays_second_and_then_the_order_resumes() {
        let mut rig = rig(vec![good("a"), good("b"), good("c"), good("d")]).await;
        rig.player.play().await.expect("the fixture plays");
        rig.player
            .enqueue(rig.list, rig.ids[3])
            .await
            .expect("the entry exists");
        assert_eq!(rig.state.borrow().queue.len(), 1);

        rig.observer.allow(FRAMES + 500);
        rig.current_is(3).await;
        assert_eq!(rig.state.borrow().queue.len(), 0);
        rig.observer.allow(FRAMES);
        rig.current_is(1).await;
        rig.observer.allow(FRAMES);
        rig.current_is(2).await;
        rig.observer.release();
        rig.stopped().await;

        assert_eq!(
            flushes(&rig.observer),
            0,
            "a track restarted instead of following"
        );
        assert_eq!(frames(&rig.observer), 5 * FRAMES);
    }

    // ---- 4: stop after current ----

    #[tokio::test]
    async fn stop_after_current_stops_at_the_end_of_the_track_and_the_flag_clears() {
        let mut rig = rig(vec![good("a"), good("b")]).await;
        rig.player.play().await.expect("the fixture plays");

        rig.player
            .set_stop_after_current(true)
            .await
            .expect("the player is running");
        assert!(rig.state.borrow().stop_after_current);
        rig.observer.release();
        rig.stopped().await;

        assert_eq!(frames(&rig.observer), FRAMES);
        until_state(&mut rig.state, "the flag to clear", |s| {
            !s.stop_after_current
        })
        .await;
    }

    // ---- 5: removing the entry in the slot ----

    #[tokio::test]
    async fn removing_the_entry_in_the_slot_refills_it_with_the_one_after() {
        let mut rig = rig(vec![good("a"), good("b"), good("c")]).await;
        rig.player.play().await.expect("the fixture plays");

        rig.player
            .remove(rig.list, vec![rig.ids[1]])
            .await
            .expect("the entry exists");
        rig.observer.allow(FRAMES + 500);
        rig.current_is(2).await;
        rig.observer.release();
        rig.stopped().await;

        assert_eq!(
            flushes(&rig.observer),
            0,
            "a track restarted instead of following"
        );
        assert_eq!(frames(&rig.observer), 2 * FRAMES);
    }

    #[tokio::test]
    async fn queueing_the_entry_that_is_next_anyway_plays_it_from_the_queue_and_then_from_the_order()
     {
        let mut rig = rig(vec![good("a"), good("b")]).await;
        rig.player.play().await.expect("the fixture plays");

        rig.player
            .enqueue(rig.list, rig.ids[1])
            .await
            .expect("the entry exists");
        rig.observer.release();
        rig.stopped().await;

        assert_eq!(frames(&rig.observer), 3 * FRAMES);
    }

    // ---- 6: next and previous at the ends ----

    async fn playing(order: PlaybackOrder) -> Rig {
        let mut rig = rig(vec![good("a"), good("b"), good("c")]).await;
        rig.player
            .set_order(order)
            .await
            .expect("the player is running");
        rig.player.play().await.expect("the fixture plays");
        rig.current_is(0).await;
        rig
    }

    #[tokio::test]
    async fn default_order_has_no_entry_before_the_first_or_after_the_last() {
        let mut rig = playing(PlaybackOrder::Default).await;

        rig.player.previous().await.expect("nothing to do is fine");
        assert_eq!(rig.state.borrow().current, Some(rig.at(0)));
        rig.player.next().await.expect("next plays");
        rig.player.next().await.expect("next plays");
        rig.current_is(2).await;
        rig.player.next().await.expect("nothing to do is fine");
        assert_eq!(rig.state.borrow().current, Some(rig.at(2)));
        rig.player.previous().await.expect("previous plays");
        rig.current_is(1).await;
        rig.player.stop().await.expect("stop is answered");
    }

    #[tokio::test]
    async fn repeat_playlist_wraps_in_both_directions() {
        let mut rig = playing(PlaybackOrder::RepeatPlaylist).await;

        rig.player.previous().await.expect("previous plays");
        rig.current_is(2).await;
        rig.player.next().await.expect("next plays");
        rig.current_is(0).await;
        rig.player.stop().await.expect("stop is answered");
    }

    #[tokio::test]
    async fn repeat_track_skips_to_neighbours_but_does_not_wrap() {
        let mut rig = playing(PlaybackOrder::RepeatTrack).await;

        rig.player.previous().await.expect("nothing to do is fine");
        assert_eq!(rig.state.borrow().current, Some(rig.at(0)));
        rig.player.next().await.expect("next plays");
        rig.player.next().await.expect("next plays");
        rig.current_is(2).await;
        rig.player.next().await.expect("nothing to do is fine");
        assert_eq!(rig.state.borrow().current, Some(rig.at(2)));
        rig.player.stop().await.expect("stop is answered");
    }

    #[tokio::test]
    async fn shuffle_goes_back_within_the_pass_and_not_before_its_start() {
        let rig = playing(PlaybackOrder::Shuffle).await;
        let first = rig.state.borrow().current.expect("playing");

        rig.player.previous().await.expect("nothing to do is fine");
        assert_eq!(rig.state.borrow().current, Some(first));
        rig.player.next().await.expect("next plays");
        let second = rig.state.borrow().current.expect("playing");
        assert_ne!(first, second);
        rig.player.previous().await.expect("previous plays");
        assert_eq!(rig.state.borrow().current, Some(first));
        rig.player.stop().await.expect("stop is answered");
    }

    // ---- 7: skipping what cannot be played ----

    #[tokio::test]
    async fn a_missing_file_between_two_good_ones_is_skipped() {
        let mut rig = rig(vec![good("a"), missing("gone"), good("c")]).await;
        rig.player.play().await.expect("the first track plays");

        rig.observer.release();
        rig.stopped().await;

        assert_eq!(frames(&rig.observer), 2 * FRAMES);
        assert_eq!(configures(&rig.observer), 1);
        assert_eq!(
            flushes(&rig.observer),
            0,
            "the skip left a gap: the output was restarted"
        );
    }

    #[tokio::test]
    async fn a_missing_file_chosen_to_start_is_skipped_for_the_next() {
        let mut rig = rig(vec![missing("gone"), good("b")]).await;

        rig.player.play().await.expect("the second track plays");

        rig.current_is(1).await;
        rig.observer.release();
        rig.stopped().await;
        assert_eq!(frames(&rig.observer), FRAMES);
    }

    #[tokio::test]
    async fn a_playlist_of_only_missing_files_under_repeat_playlist_stops() {
        let rig = rig(vec![missing("one"), missing("two")]).await;
        rig.player
            .set_order(PlaybackOrder::RepeatPlaylist)
            .await
            .expect("the player is running");

        let result = timeout(PATIENCE, rig.player.play())
            .await
            .expect("it gave up instead of spinning");

        assert!(matches!(result, Err(PlayerError::Engine(_))), "{result:?}");
        assert!(rig.state.borrow().current.is_none());
        assert!(matches!(*rig.playback.borrow(), PlaybackState::Stopped));
    }

    #[tokio::test]
    async fn failures_in_a_row_stop_a_repeating_playlist_that_loses_its_files_midway() {
        let mut rig = rig(vec![good("a"), missing("one"), missing("two")]).await;
        rig.player
            .set_order(PlaybackOrder::RepeatPlaylist)
            .await
            .expect("the player is running");
        rig.player.play().await.expect("the first track plays");

        rig.observer.allow(3 * FRAMES);
        rig.current_is(0).await;
        tokio::time::sleep(Duration::from_millis(100)).await;

        assert_eq!(rig.state.borrow().current, Some(rig.at(0)));
        rig.player.stop().await.expect("stop is answered");
    }

    #[tokio::test]
    async fn starting_from_stopped_plays_the_queue_first_and_uses_it_up() {
        let mut rig = rig(vec![good("a"), good("b")]).await;
        rig.player
            .enqueue(rig.list, rig.ids[1])
            .await
            .expect("it exists");

        rig.player.play().await.expect("the fixture plays");

        rig.current_is(1).await;
        assert_eq!(rig.state.borrow().queue.len(), 0);
        rig.player.stop().await.expect("stop is answered");
    }

    #[tokio::test]
    async fn exactly_as_many_failures_in_a_row_as_the_playlist_has_entries_stop_the_skipping() {
        let rig = rig(vec![missing("one"), missing("two")]).await;
        let other = rig
            .player
            .create_playlist("other".to_owned())
            .await
            .expect("it is created");
        let fine = rig
            .player
            .append(other, vec![good("fine")])
            .await
            .expect("it exists")[0];
        for entry in [rig.ids[0], rig.ids[1]] {
            rig.player.enqueue(rig.list, entry).await.expect("exists");
        }
        rig.player.enqueue(other, fine).await.expect("exists");

        let result = rig.player.play().await;

        assert!(matches!(result, Err(PlayerError::Engine(_))), "{result:?}");
        assert!(rig.state.borrow().current.is_none());
        assert_eq!(
            rig.state.borrow().queue.len(),
            1,
            "the good entry was tried"
        );
        assert!(matches!(*rig.playback.borrow(), PlaybackState::Stopped));
    }

    // ---- 8: pause and seek ----

    #[tokio::test]
    async fn toggling_pause_twice_pauses_then_resumes() {
        let mut rig = rig(vec![good("a")]).await;
        rig.player.play().await.expect("the fixture plays");
        rig.observer.allow(5_000);
        until_playback(&mut rig.playback, "playing", |s| {
            transport(s) == Some(Transport::Playing)
        })
        .await;

        rig.player.toggle_pause().await.expect("pause is answered");
        assert_eq!(transport(&rig.playback.borrow()), Some(Transport::Paused));
        rig.player.toggle_pause().await.expect("resume is answered");
        until_playback(&mut rig.playback, "playing again", |s| {
            transport(s) == Some(Transport::Playing)
        })
        .await;
        rig.player.stop().await.expect("stop is answered");
    }

    #[tokio::test]
    async fn playing_again_after_the_playlist_finished_starts_it_from_the_top() {
        let mut rig = rig(vec![good("a"), good("b")]).await;
        rig.player.play().await.expect("the fixture plays");
        rig.observer.release();
        rig.stopped().await;
        rig.observer.hold();

        rig.player.play().await.expect("it starts over");

        rig.current_is(0).await;
        rig.player.stop().await.expect("stop is answered");
    }

    #[tokio::test]
    async fn toggling_pause_when_stopped_starts_playing() {
        let mut rig = rig(vec![good("a")]).await;

        rig.player.toggle_pause().await.expect("it starts");

        rig.current_is(0).await;
        rig.player.stop().await.expect("stop is answered");
    }

    #[tokio::test]
    async fn play_while_paused_resumes_and_while_playing_does_nothing() {
        let mut rig = rig(vec![good("a")]).await;
        rig.player.play().await.expect("the fixture plays");
        rig.observer.allow(5_000);
        until_playback(&mut rig.playback, "playing", |s| {
            transport(s) == Some(Transport::Playing)
        })
        .await;

        rig.player.play().await.expect("playing already is fine");
        rig.player.toggle_pause().await.expect("pause is answered");
        rig.player.play().await.expect("resume is answered");

        until_playback(&mut rig.playback, "playing again", |s| {
            transport(s) == Some(Transport::Playing)
        })
        .await;
        rig.player.stop().await.expect("stop is answered");
    }

    async fn landed_near(rig: &mut Rig, target: Duration) {
        let from = target.saturating_sub(Duration::from_millis(120));
        until_playback(&mut rig.playback, "the seek to land", |s| {
            position(s).is_some_and(|at| at >= from && at <= target)
        })
        .await;
    }

    #[tokio::test]
    async fn seek_to_forward_and_back_land_near_their_targets() {
        let mut rig = rig(vec![good("a")]).await;
        rig.player.play().await.expect("the fixture plays");

        rig.player
            .seek(Seek::To(Duration::from_millis(500)))
            .await
            .expect("the fixture seeks");
        landed_near(&mut rig, Duration::from_millis(500)).await;
        rig.player
            .seek(Seek::Forward(Duration::from_millis(200)))
            .await
            .expect("the fixture seeks");
        landed_near(&mut rig, Duration::from_millis(700)).await;
        rig.player
            .seek(Seek::Back(Duration::from_millis(400)))
            .await
            .expect("the fixture seeks");
        landed_near(&mut rig, Duration::from_millis(300)).await;
        rig.player.stop().await.expect("stop is answered");
    }

    #[tokio::test]
    async fn seeking_back_past_the_start_goes_to_the_start() {
        let mut rig = rig(vec![good("a")]).await;
        rig.player.play().await.expect("the fixture plays");
        rig.player
            .seek(Seek::To(Duration::from_millis(500)))
            .await
            .expect("the fixture seeks");

        rig.player
            .seek(Seek::Back(Duration::from_secs(3_600)))
            .await
            .expect("it clamps to the start");

        until_playback(&mut rig.playback, "the start", |s| {
            position(s) == Some(Duration::ZERO)
        })
        .await;
        rig.player.stop().await.expect("stop is answered");
    }

    #[tokio::test]
    async fn seeking_past_the_end_is_an_error_and_playback_carries_on() {
        let rig = rig(vec![good("a")]).await;
        rig.player.play().await.expect("the fixture plays");

        let forward = rig
            .player
            .seek(Seek::Forward(Duration::from_secs(3_600)))
            .await;
        let to = rig.player.seek(Seek::To(Duration::from_secs(3_600))).await;

        assert!(
            matches!(forward, Err(PlayerError::Engine(_))),
            "{forward:?}"
        );
        assert!(matches!(to, Err(PlayerError::Engine(_))), "{to:?}");
        assert!(matches!(
            *rig.playback.borrow(),
            PlaybackState::Active { .. }
        ));
        rig.player.stop().await.expect("stop is answered");
    }

    #[tokio::test]
    async fn seeking_while_stopped_is_an_error() {
        let rig = rig(vec![good("a")]).await;

        let result = rig.player.seek(Seek::To(Duration::ZERO)).await;

        assert!(matches!(result, Err(PlayerError::Engine(_))), "{result:?}");
    }

    // ---- 9: shutdown ----

    #[tokio::test]
    async fn dropping_every_handle_joins_both_threads() {
        let rig = rig(vec![good("a")]).await;
        rig.player.play().await.expect("the fixture plays");
        let clone = rig.player.clone();
        let mut state = rig.state.clone();
        let mut playback = rig.playback.clone();
        let Rig { player, .. } = rig;
        drop(clone);

        timeout(PATIENCE, tokio::task::spawn_blocking(move || drop(player)))
            .await
            .expect("both threads joined in time")
            .expect("dropping the player did not panic");

        timeout(PATIENCE, async {
            while state.changed().await.is_ok() {}
            while playback.changed().await.is_ok() {}
        })
        .await
        .expect("the threads released their channels");
        assert!(matches!(*playback.borrow(), PlaybackState::Stopped));
    }

    // ---- everything else the handle does ----

    #[tokio::test]
    async fn a_command_naming_something_that_is_gone_is_an_error() {
        let rig = rig(vec![good("a")]).await;
        rig.player
            .remove(rig.list, vec![rig.ids[0]])
            .await
            .expect("it exists");

        let enqueue = rig.player.enqueue(rig.list, rig.ids[0]).await;
        let play = rig.player.play_entry(rig.list, rig.ids[0]).await;
        let append = rig
            .player
            .append(
                {
                    let other = Player::with_fake_sink(
                        FakeSink::new(NonZeroUsize::new(CAPACITY).expect("non-zero")),
                        1,
                    )
                    .expect("starts");
                    other.create_playlist("x".to_owned()).await.expect("runs");
                    other.create_playlist("y".to_owned()).await.expect("runs")
                },
                vec![],
            )
            .await;

        assert!(
            matches!(enqueue, Err(PlayerError::Sequence(_))),
            "{enqueue:?}"
        );
        assert!(matches!(play, Err(PlayerError::Sequence(_))), "{play:?}");
        assert!(
            matches!(append, Err(PlayerError::Sequence(_))),
            "{append:?}"
        );
    }

    #[tokio::test]
    async fn playing_with_nothing_to_play_is_an_error() {
        let rig = rig(vec![]).await;

        let result = rig.player.play().await;

        assert!(
            matches!(result, Err(PlayerError::NothingToPlay)),
            "{result:?}"
        );
    }

    #[tokio::test]
    async fn play_entry_starts_that_entry_and_the_order_continues_from_it() {
        let mut rig = rig(vec![good("a"), good("b"), good("c")]).await;

        rig.player
            .play_entry(rig.list, rig.ids[1])
            .await
            .expect("the fixture plays");

        rig.current_is(1).await;
        rig.observer.release();
        rig.current_is(2).await;
        rig.stopped().await;
        assert_eq!(frames(&rig.observer), 2 * FRAMES);
    }

    #[tokio::test]
    async fn stopping_clears_what_is_playing_and_keeps_the_playlists_and_the_queue() {
        let rig = rig(vec![good("a"), good("b")]).await;
        rig.player.play().await.expect("the fixture plays");
        rig.player
            .enqueue(rig.list, rig.ids[1])
            .await
            .expect("it exists");

        rig.player.stop().await.expect("stop is answered");

        assert!(rig.state.borrow().current.is_none());
        assert_eq!(rig.state.borrow().queue.len(), 1);
        assert_eq!(rig.state.borrow().playlists.len(), 1);
        rig.observer.release();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(frames(&rig.observer), 0);
    }

    #[tokio::test]
    async fn volume_and_replaygain_reach_the_output_and_the_state() {
        let mut rig = rig(vec![good("a")]).await;
        let half = Volume::from_linear(0.5).expect("in range");

        rig.player.set_volume(half).await.expect("it is answered");
        rig.player
            .set_replay_gain(ReplayGainMode::Off)
            .await
            .expect("it is answered");
        rig.player.play().await.expect("the fixture plays");
        rig.observer.release();
        rig.stopped().await;

        let state = rig.state.borrow().clone();
        assert_eq!(state.volume, half);
        assert_eq!(state.replay_gain, ReplayGainMode::Off);
        let peak = rig
            .observer
            .played()
            .iter()
            .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
        assert!(
            peak > 0.02 && peak < 0.05,
            "peak {peak} is not half of ~0.088"
        );
    }

    #[tokio::test]
    async fn the_state_shows_edits_as_soon_as_their_reply_arrives() {
        let rig = rig(vec![good("a"), good("b")]).await;

        rig.player
            .move_before(rig.list, rig.ids[1], Some(rig.ids[0]))
            .await
            .expect("both exist");
        let other = rig
            .player
            .create_playlist("other".to_owned())
            .await
            .expect("it is created");
        rig.player.set_active(other).await.expect("it exists");
        rig.player
            .set_order(PlaybackOrder::Shuffle)
            .await
            .expect("it is answered");

        let state = rig.state.borrow().clone();
        assert_eq!(state.playlists.len(), 2);
        assert_eq!(state.active, Some(other));
        assert_eq!(state.order, PlaybackOrder::Shuffle);
        let first = &state.playlists[0];
        assert_eq!(first.entries()[0].id, rig.ids[1]);
    }
}
