//! What the keys change, as plain arithmetic.

use std::time::Duration;

use aeri_core::Volume;
use aeri_player::{PlaybackOrder, Player, PlayerError, PlayerState, Seek};

use crate::keys::Action;

/// Does what `action` asks of the player. `state` is the player's state when the key was
/// pressed, which the relative actions (volume, order, stop-after-current) start from.
///
/// [`Action::Quit`] is for the caller: it asks the player for nothing.
pub(crate) async fn apply(
    action: Action,
    player: &Player,
    state: &PlayerState,
) -> Result<(), PlayerError> {
    match action {
        Action::TogglePause => player.toggle_pause().await,
        Action::Next => player.next().await,
        Action::Previous => player.previous().await,
        Action::SeekForward => player.seek(Seek::Forward(SEEK_STEP)).await,
        Action::SeekBack => player.seek(Seek::Back(SEEK_STEP)).await,
        Action::VolumeUp => {
            player
                .set_volume(step_volume(state.volume, VOLUME_STEP_DB))
                .await
        }
        Action::VolumeDown => {
            player
                .set_volume(step_volume(state.volume, -VOLUME_STEP_DB))
                .await
        }
        Action::CycleOrder => player.set_order(next_order(state.order)).await,
        Action::ToggleStopAfterCurrent => {
            player
                .set_stop_after_current(!state.stop_after_current)
                .await
        }
        Action::Quit => Ok(()),
    }
}

/// How far the left and right keys seek.
pub(crate) const SEEK_STEP: Duration = Duration::from_secs(5);

/// How much the up and down keys change the volume, in decibels.
pub(crate) const VOLUME_STEP_DB: f32 = 2.0;

/// Below this the next step down is silence, and the first step up from silence lands here.
const FLOOR_DB: f32 = -60.0;

/// The volume `delta_db` decibels above `current`, clamped to full volume at the top, and to
/// silence below [`FLOOR_DB`], from which it can still climb.
pub(crate) fn step_volume(current: Volume, delta_db: f32) -> Volume {
    let db = current.db().max(FLOOR_DB) + delta_db;
    if db < FLOOR_DB {
        Volume::SILENT
    } else {
        Volume::from_db(db)
    }
}

/// The order after `order` when cycling: default, repeat track, repeat playlist, shuffle.
pub(crate) fn next_order(order: PlaybackOrder) -> PlaybackOrder {
    match order {
        PlaybackOrder::Default => PlaybackOrder::RepeatTrack,
        PlaybackOrder::RepeatTrack => PlaybackOrder::RepeatPlaylist,
        PlaybackOrder::RepeatPlaylist => PlaybackOrder::Shuffle,
        PlaybackOrder::Shuffle | _ => PlaybackOrder::Default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db(volume: Volume) -> f32 {
        volume.db()
    }

    #[test]
    fn stepping_down_lowers_the_volume_by_the_step() {
        let lowered = step_volume(Volume::UNITY, -VOLUME_STEP_DB);

        assert!((db(lowered) + 2.0).abs() < 1e-4, "{}", db(lowered));
        assert!((db(step_volume(lowered, -VOLUME_STEP_DB)) + 4.0).abs() < 1e-4);
    }

    #[test]
    fn stepping_up_from_below_full_raises_it_and_stops_at_full() {
        let quiet = Volume::from_db(-10.0);

        assert!((db(step_volume(quiet, VOLUME_STEP_DB)) + 8.0).abs() < 1e-4);
        assert_eq!(step_volume(Volume::UNITY, VOLUME_STEP_DB), Volume::UNITY);
        assert_eq!(
            step_volume(Volume::from_db(-1.0), VOLUME_STEP_DB),
            Volume::UNITY
        );
    }

    #[test]
    fn below_the_floor_the_next_step_down_is_silence() {
        assert_eq!(
            step_volume(Volume::from_db(-59.0), -VOLUME_STEP_DB),
            Volume::SILENT
        );
        assert_eq!(
            step_volume(Volume::from_db(-60.0), -VOLUME_STEP_DB),
            Volume::SILENT
        );
        assert!((db(step_volume(Volume::from_db(-57.0), -VOLUME_STEP_DB)) + 59.0).abs() < 1e-3);
    }

    #[test]
    fn a_step_that_lands_exactly_on_the_floor_is_still_a_sound() {
        assert!(
            db(Volume::from_db(-60.0)).to_bits() == (-60.0_f32).to_bits(),
            "{}",
            db(Volume::from_db(-60.0))
        );
        assert_ne!(step_volume(Volume::from_db(-60.0), 0.0), Volume::SILENT);
    }

    #[test]
    fn silence_can_be_climbed_out_of_and_stays_silent_going_down() {
        let first = step_volume(Volume::SILENT, VOLUME_STEP_DB);

        assert!((db(first) + 58.0).abs() < 1e-3, "{}", db(first));
        assert_eq!(step_volume(Volume::SILENT, -VOLUME_STEP_DB), Volume::SILENT);
    }

    #[test]
    fn every_order_leads_to_the_next_and_the_cycle_closes() {
        let mut order = PlaybackOrder::Default;
        let mut seen = Vec::new();
        for _ in 0..4 {
            order = next_order(order);
            seen.push(order);
        }

        assert_eq!(
            seen,
            [
                PlaybackOrder::RepeatTrack,
                PlaybackOrder::RepeatPlaylist,
                PlaybackOrder::Shuffle,
                PlaybackOrder::Default,
            ]
        );
    }

    #[test]
    fn the_steps_are_the_ones_the_brief_names() {
        assert_eq!(SEEK_STEP, Duration::from_secs(5));
        assert!((VOLUME_STEP_DB - 2.0).abs() < f32::EPSILON);
    }

    // ---- apply, against a real player over the fake sink ----

    mod with_a_player {
        use std::num::NonZeroUsize;
        use std::path::PathBuf;
        use std::sync::Arc;

        use aeri_core::{PlaybackState, Source, Track, Transport};
        use aeri_player::{FakeSink, FakeSinkObserver, PlaylistId};
        use tokio::sync::watch;
        use tokio::time::timeout;

        use super::*;

        const PATIENCE: Duration = Duration::from_secs(10);

        struct Rig {
            player: Player,
            observer: FakeSinkObserver,
            roster: watch::Receiver<PlayerState>,
            playback: watch::Receiver<PlaybackState>,
            list: PlaylistId,
        }

        impl Rig {
            fn state(&self) -> PlayerState {
                self.roster.borrow().clone()
            }

            async fn apply(&self, action: Action) -> Result<(), PlayerError> {
                apply(action, &self.player, &self.state()).await
            }

            async fn playing(&mut self) {
                timeout(
                    PATIENCE,
                    self.playback.wait_for(|state| {
                        matches!(
                            state,
                            PlaybackState::Active {
                                transport: Transport::Playing,
                                ..
                            }
                        )
                    }),
                )
                .await
                .expect("it started playing in time")
                .expect("the engine is running");
            }

            fn transport(&self) -> Option<Transport> {
                match &*self.playback.borrow() {
                    PlaybackState::Active { transport, .. } => Some(*transport),
                    _ => None,
                }
            }
        }

        fn song(name: &str) -> Arc<Track> {
            let path = PathBuf::from(name);
            Arc::new(Track {
                source: Source::File(path),
                duration: None,
                title: None,
                artist: None,
                album: None,
            })
        }

        async fn rig(count: usize) -> Rig {
            let sink = FakeSink::new(NonZeroUsize::new(4_096).expect("non-zero"));
            let observer = sink.observer();
            observer.hold();
            let player = Player::with_fake_sink(sink, 1).expect("the threads start");
            let roster = player.subscribe();
            let playback = player.playback();
            let list = player
                .create_playlist("list".to_owned())
                .await
                .expect("running");
            player
                .append(list, (0..count).map(|_| song("sine.flac")).collect())
                .await
                .expect("exists");
            Rig {
                player,
                observer,
                roster,
                playback,
                list,
            }
        }

        #[tokio::test]
        async fn the_volume_keys_step_the_players_volume_down_and_up() {
            let rig = rig(1).await;

            rig.apply(Action::VolumeDown).await.expect("applied");
            rig.apply(Action::VolumeDown).await.expect("applied");
            assert!((rig.state().volume.db() + 4.0).abs() < 1e-3);
            rig.apply(Action::VolumeUp).await.expect("applied");
            assert!((rig.state().volume.db() + 2.0).abs() < 1e-3);
        }

        #[tokio::test]
        async fn the_order_key_cycles_the_players_order() {
            let rig = rig(1).await;

            rig.apply(Action::CycleOrder).await.expect("applied");
            assert_eq!(rig.state().order, PlaybackOrder::RepeatTrack);
            rig.apply(Action::CycleOrder).await.expect("applied");
            assert_eq!(rig.state().order, PlaybackOrder::RepeatPlaylist);
        }

        #[tokio::test]
        async fn the_stop_key_toggles_stop_after_current_on_and_off() {
            let rig = rig(1).await;

            rig.apply(Action::ToggleStopAfterCurrent)
                .await
                .expect("applied");
            assert!(rig.state().stop_after_current);
            rig.apply(Action::ToggleStopAfterCurrent)
                .await
                .expect("applied");
            assert!(!rig.state().stop_after_current);
        }

        #[tokio::test]
        async fn the_space_key_pauses_and_resumes() {
            let mut rig = rig(1).await;
            rig.player.play().await.expect("plays");
            rig.observer.allow(5_000);
            rig.playing().await;

            rig.apply(Action::TogglePause).await.expect("applied");
            assert_eq!(rig.transport(), Some(Transport::Paused));
            rig.apply(Action::TogglePause).await.expect("applied");
            rig.playing().await;
        }

        #[tokio::test]
        async fn the_skip_keys_move_through_the_playlist() {
            let rig = rig(3).await;
            rig.player.play().await.expect("plays");
            let at = |index: usize| rig.state().playlists[0].entries()[index].id;
            let current = |rig: &Rig| rig.state().current.map(|position| position.entry);
            assert_eq!(current(&rig), Some(at(0)));

            rig.apply(Action::Next).await.expect("applied");
            rig.apply(Action::Next).await.expect("applied");
            assert_eq!(current(&rig), Some(at(2)));
            rig.apply(Action::Previous).await.expect("applied");
            assert_eq!(current(&rig), Some(at(1)));
            assert_eq!(rig.state().active, Some(rig.list));
        }

        #[tokio::test]
        async fn the_arrow_keys_seek_forward_and_back_by_five_seconds() {
            let rig = rig(1).await;
            rig.player.play().await.expect("plays");

            // The fixture is one second long: five seconds ahead is past its end, five back
            // clamps to its start.
            assert!(rig.apply(Action::SeekForward).await.is_err());
            assert!(rig.apply(Action::SeekBack).await.is_ok());
        }

        #[tokio::test]
        async fn quit_asks_nothing_of_the_player() {
            let rig = rig(1).await;

            rig.apply(Action::Quit).await.expect("applied");

            assert_eq!(rig.state().current, None);
        }
    }
}
