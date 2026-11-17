//! The `aeri` command-line player. It depends on `aeri-player` and `aeri-core` only (ADR 0006).

mod args;
mod output;
mod paths;

use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use aeri_core::{PlaybackState, Source, Track, Volume};
use aeri_player::Player;
use anyhow::{Context as _, Result};
use clap::Parser as _;
use tracing::Level;

use crate::args::{Cli, Command, Gain, Order};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_max_level(Level::WARN)
        .init();
    match Cli::parse().command {
        Command::Play {
            paths,
            order,
            volume,
            replaygain,
        } => play(&paths, order, volume, replaygain).await,
    }
}

fn track(path: PathBuf) -> Arc<Track> {
    Arc::new(Track {
        source: Source::File(path),
        duration: None,
        title: None,
        artist: None,
        album: None,
    })
}

/// Playback is over when the engine is stopped and the player has nothing current. The engine
/// is stopped for a moment between a track that failed and the one the player starts next,
/// so the engine alone would end the loop early.
fn is_over(playback: &PlaybackState, player_has_current: bool) -> bool {
    matches!(playback, PlaybackState::Stopped) && !player_has_current
}

/// The track to announce: the one playing now, if it is not the one already shown.
fn newly_started<'a>(
    shown: Option<&Arc<Track>>,
    playback: &'a PlaybackState,
) -> Option<&'a Arc<Track>> {
    match playback {
        PlaybackState::Active { track, .. }
            if !shown.is_some_and(|shown| Arc::ptr_eq(shown, track)) =>
        {
            Some(track)
        }
        _ => None,
    }
}

async fn play(
    inputs: &[PathBuf],
    order: Order,
    volume: Option<f32>,
    replaygain: Option<Gain>,
) -> Result<()> {
    // Report a bad path before opening the audio device.
    let files = paths::expand(inputs)?;

    let player = Player::with_default_output().context("cannot start playback")?;
    let mut playback = player.playback();
    let mut roster = player.subscribe();
    player.set_order(order.into()).await?;
    if let Some(db) = volume {
        player.set_volume(Volume::from_db(db)).await?;
    }
    if let Some(gain) = replaygain {
        player.set_replay_gain(gain.into()).await?;
    }
    let playlist = player.create_playlist("play".to_owned()).await?;
    player
        .append(playlist, files.into_iter().map(track).collect())
        .await?;
    player.play().await.context("nothing could be played")?;

    let mut shown: Option<Arc<Track>> = None;
    let mut stdout = io::stdout();
    loop {
        let current = playback.borrow_and_update().clone();
        let playing = roster.borrow_and_update().current.is_some();
        if is_over(&current, playing) {
            break;
        }
        if let Some(track) = newly_started(shown.as_ref(), &current) {
            if shown.is_some() {
                output::finish(&mut stdout)?;
            }
            output::now_playing(&mut stdout, track)?;
            shown = Some(Arc::clone(track));
        }
        output::progress(&mut stdout, &current)?;

        tokio::select! {
            changed = playback.changed() => {
                if changed.is_err() {
                    break;
                }
            }
            changed = roster.changed() => {
                if changed.is_err() {
                    break;
                }
            }
            interrupted = tokio::signal::ctrl_c() => {
                interrupted.context("cannot listen for Ctrl-C")?;
                player.stop().await.context("cannot stop playback")?;
                break;
            }
        }
    }
    output::finish(&mut stdout)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use aeri_core::Transport;

    use super::*;

    fn active(track: &Arc<Track>) -> PlaybackState {
        PlaybackState::Active {
            track: Arc::clone(track),
            transport: Transport::Playing,
            position: Duration::ZERO,
            output: None,
        }
    }

    #[test]
    fn playback_is_over_only_when_the_engine_is_stopped_and_the_player_has_nothing_current() {
        let song = track(PathBuf::from("a.flac"));

        assert!(is_over(&PlaybackState::Stopped, false));
        assert!(!is_over(&PlaybackState::Stopped, true));
        assert!(!is_over(&active(&song), false));
        assert!(!is_over(&active(&song), true));
    }

    #[test]
    fn a_track_is_announced_once_and_again_when_a_different_one_starts() {
        let first = track(PathBuf::from("a.flac"));
        let second = track(PathBuf::from("a.flac"));

        let (playing_first, playing_second) = (active(&first), active(&second));

        let at_start = newly_started(None, &playing_first).expect("the first is announced");
        assert!(Arc::ptr_eq(at_start, &first));
        assert!(newly_started(Some(&first), &playing_first).is_none());
        let next = newly_started(Some(&first), &playing_second).expect("a new offer is announced");
        assert!(Arc::ptr_eq(next, &second));
        assert!(newly_started(Some(&first), &PlaybackState::Stopped).is_none());
        assert!(newly_started(None, &PlaybackState::Stopped).is_none());
    }
}
