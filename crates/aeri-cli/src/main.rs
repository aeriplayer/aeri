//! The `aeri` command-line player.
//!
//! Depends on `aeri-engine` directly until `aeri-player` exists (ADR 0006); then it moves
//! onto the `Player` handle and the `deny.toml` exception closes.

mod output;

use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use aeri_core::{PlaybackState, Source, Track};
use aeri_engine::{Engine, EngineEvent};
use anyhow::{Context as _, Result};
use clap::{Parser, Subcommand};
use tracing::Level;

#[derive(Debug, Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Play a FLAC or MP3 file through the default output device.
    Play {
        /// The file to play.
        path: PathBuf,
    },
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_max_level(Level::WARN)
        .init();
    match Cli::parse().command {
        Command::Play { path } => play(path).await,
    }
}

async fn play(path: PathBuf) -> Result<()> {
    // Report a bad path before opening the audio device.
    fs_err::metadata(&path).with_context(|| format!("cannot play {}", path.display()))?;
    // Held, not read: the player takes over the events when this command moves onto it.
    let (events, _events) = tokio::sync::mpsc::unbounded_channel::<EngineEvent>();
    let engine = Engine::with_default_output(events).context("cannot start playback")?;
    let mut state = engine.subscribe();
    let track = Arc::new(Track {
        source: Source::File(path.clone()),
        duration: None,
        title: None,
        artist: None,
        album: None,
    });
    engine
        .play(track)
        .await
        .with_context(|| format!("cannot play {}", path.display()))?;

    loop {
        tokio::select! {
            changed = state.changed() => {
                if changed.is_err() {
                    break;
                }
                let current = state.borrow_and_update().clone();
                if matches!(current, PlaybackState::Stopped) {
                    break;
                }
                output::progress(&mut io::stdout(), &current)?;
            }
            interrupted = tokio::signal::ctrl_c() => {
                interrupted.context("cannot listen for Ctrl-C")?;
                engine.stop().await.context("cannot stop playback")?;
                break;
            }
        }
    }
    output::finish(&mut io::stdout())?;
    Ok(())
}
