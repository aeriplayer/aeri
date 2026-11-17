//! The command line, as `clap` reads it.

use std::path::PathBuf;

use aeri_core::ReplayGainMode;
use aeri_player::PlaybackOrder;
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(version, about)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Play FLAC and MP3 files through the default output device.
    Play {
        /// Files to play, directories to search for them, and `.m3u` or `.m3u8` playlists.
        #[arg(required = true, value_name = "PATH")]
        paths: Vec<PathBuf>,

        /// How the files are played through.
        #[arg(long, value_enum, default_value_t = Order::Default)]
        order: Order,

        /// Output volume, in decibels below full volume, such as -6. Above 0 is full volume.
        #[arg(long, allow_negative_numbers = true, value_parser = decibels, value_name = "DB")]
        volume: Option<f32>,

        /// Which `ReplayGain` value to apply. Albums by default.
        #[arg(long, value_enum)]
        replaygain: Option<Gain>,
    },
}

/// `--order`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Order {
    /// Each file once, in order.
    Default,
    /// The same file again and again.
    RepeatTrack,
    /// All the files, again and again.
    RepeatPlaylist,
    /// Every file once, in a random order, and again.
    Shuffle,
}

impl From<Order> for PlaybackOrder {
    fn from(order: Order) -> Self {
        match order {
            Order::Default => Self::Default,
            Order::RepeatTrack => Self::RepeatTrack,
            Order::RepeatPlaylist => Self::RepeatPlaylist,
            Order::Shuffle => Self::Shuffle,
        }
    }
}

/// `--replaygain`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Gain {
    /// Apply none.
    Off,
    /// Level each file alone.
    Track,
    /// Keep an album's balance.
    Album,
}

impl From<Gain> for ReplayGainMode {
    fn from(gain: Gain) -> Self {
        match gain {
            Gain::Off => Self::Off,
            Gain::Track => Self::Track,
            Gain::Album => Self::Album,
        }
    }
}

fn decibels(text: &str) -> Result<f32, String> {
    text.parse::<f32>()
        .ok()
        .filter(|db| db.is_finite())
        .ok_or_else(|| format!("`{text}` is not a number of decibels"))
}

#[cfg(test)]
mod tests {
    use super::*;

    type Parsed = (Vec<PathBuf>, Order, Option<f32>, Option<Gain>);

    fn play(arguments: &[&str]) -> Result<Parsed, clap::Error> {
        let mut line = vec!["aeri", "play"];
        line.extend(arguments);
        Cli::try_parse_from(line).map(|cli| match cli.command {
            Command::Play {
                paths,
                order,
                volume,
                replaygain,
            } => (paths, order, volume, replaygain),
        })
    }

    #[test]
    fn several_paths_and_no_flags_use_the_defaults() {
        let (paths, order, volume, gain) = play(&["a.flac", "dir", "list.m3u"]).expect("parses");

        assert_eq!(
            paths,
            [
                PathBuf::from("a.flac"),
                PathBuf::from("dir"),
                PathBuf::from("list.m3u")
            ]
        );
        assert_eq!(order, Order::Default);
        assert_eq!(volume, None);
        assert_eq!(gain, None);
    }

    #[test]
    fn every_order_is_spelled_as_its_variant() {
        for (name, expected) in [
            ("default", Order::Default),
            ("repeat-track", Order::RepeatTrack),
            ("repeat-playlist", Order::RepeatPlaylist),
            ("shuffle", Order::Shuffle),
        ] {
            let (_, order, _, _) = play(&["--order", name, "a.flac"]).expect("parses");
            assert_eq!(order, expected, "{name}");
        }
    }

    #[test]
    fn every_replaygain_mode_is_spelled_as_its_variant() {
        for (name, expected) in [
            ("off", Gain::Off),
            ("track", Gain::Track),
            ("album", Gain::Album),
        ] {
            let (_, _, _, gain) = play(&["--replaygain", name, "a.flac"]).expect("parses");
            assert_eq!(gain, Some(expected), "{name}");
        }
    }

    #[test]
    fn the_flags_map_onto_the_players_types() {
        assert_eq!(PlaybackOrder::from(Order::Default), PlaybackOrder::Default);
        assert_eq!(
            PlaybackOrder::from(Order::RepeatTrack),
            PlaybackOrder::RepeatTrack
        );
        assert_eq!(
            PlaybackOrder::from(Order::RepeatPlaylist),
            PlaybackOrder::RepeatPlaylist
        );
        assert_eq!(PlaybackOrder::from(Order::Shuffle), PlaybackOrder::Shuffle);
        assert_eq!(ReplayGainMode::from(Gain::Off), ReplayGainMode::Off);
        assert_eq!(ReplayGainMode::from(Gain::Track), ReplayGainMode::Track);
        assert_eq!(ReplayGainMode::from(Gain::Album), ReplayGainMode::Album);
    }

    #[test]
    fn a_negative_volume_is_a_value_not_a_flag() {
        let (paths, _, volume, _) = play(&["--volume", "-6.5", "a.flac"]).expect("parses");

        assert_eq!(volume, Some(-6.5));
        assert_eq!(paths, [PathBuf::from("a.flac")]);
        assert_eq!(
            play(&["--volume=-3", "a.flac"]).expect("parses").2,
            Some(-3.0)
        );
    }

    #[test]
    fn a_volume_that_is_not_a_finite_number_is_refused() {
        for bad in ["loud", "nan", "inf", ""] {
            let error = play(&["--volume", bad, "a.flac"]).expect_err(bad);
            assert!(error.to_string().contains("decibels"), "{bad}: {error}");
        }
        assert!(play(&["--volume=-inf", "a.flac"]).is_err());
    }

    #[test]
    fn playing_nothing_is_a_usage_error() {
        let error = play(&[]).expect_err("a path is required");

        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn an_unknown_order_is_refused() {
        assert!(play(&["--order", "sideways", "a.flac"]).is_err());
    }
}
