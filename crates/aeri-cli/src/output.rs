//! Everything `aeri` shows the user, written to whatever the caller passes (stdout in
//! `main`, a buffer in tests). Diagnostics go to stderr through `tracing`.

use std::io::{self, Write};
use std::time::Duration;

use aeri_core::{PlaybackState, Source, Track, Transport, Volume};
use aeri_player::{PlaybackOrder, PlayerState, Playlist, Position};

/// How lines end, and whether the progress line may clear what a longer one left behind.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Style {
    newline: &'static str,
    clear_rest: &'static str,
}

impl Style {
    /// For output that is not driving a terminal: plain newlines and no control codes.
    pub(crate) const PLAIN: Self = Self {
        newline: "\n",
        clear_rest: "",
    };

    /// For a terminal in raw mode, which does not turn a newline into a carriage return and a
    /// line feed, and which can erase to the end of the line.
    pub(crate) const RAW_TERMINAL: Self = Self {
        newline: "\r\n",
        clear_rest: "\x1b[K",
    };
}

/// What the progress line says besides the transport and the clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Status {
    /// The entry playing, counted from 1, and how many the playlist has.
    pub(crate) place: Option<(usize, usize)>,
    pub(crate) order: PlaybackOrder,
    pub(crate) volume: Volume,
    pub(crate) stop_after_current: bool,
}

impl Status {
    pub(crate) fn of(state: &PlayerState) -> Self {
        Self {
            place: place(&state.playlists, state.current),
            order: state.order,
            volume: state.volume,
            stop_after_current: state.stop_after_current,
        }
    }
}

/// Where `current` is in its playlist: its place from 1, and the playlist's length.
fn place(
    playlists: &[std::sync::Arc<Playlist>],
    current: Option<Position>,
) -> Option<(usize, usize)> {
    let current = current?;
    let playlist = playlists
        .iter()
        .find(|list| list.id() == current.playlist)?;
    let index = playlist
        .entries()
        .iter()
        .position(|entry| entry.id == current.entry)?;
    Some((index + 1, playlist.len()))
}

fn order_name(order: PlaybackOrder) -> &'static str {
    match order {
        PlaybackOrder::Default => "default",
        PlaybackOrder::RepeatTrack => "repeat-track",
        PlaybackOrder::RepeatPlaylist => "repeat-playlist",
        PlaybackOrder::Shuffle => "shuffle",
        _ => "unknown order",
    }
}

fn volume_text(volume: Volume) -> String {
    if volume == Volume::SILENT {
        "muted".to_owned()
    } else {
        format!("{:.1} dB", volume.db())
    }
}

/// Rewrites the progress line in place.
pub(crate) fn progress(
    out: &mut impl Write,
    style: Style,
    state: &PlaybackState,
    status: &Status,
) -> io::Result<()> {
    if let PlaybackState::Active {
        transport,
        position,
        ..
    } = state
    {
        let label = match transport {
            Transport::Buffering => "buffering",
            Transport::Playing => "playing",
            Transport::Paused => "paused",
        };
        write!(out, "\r{label:<9} {}", clock(*position))?;
        if let Some((at, of)) = status.place {
            write!(out, "  {at} of {of}")?;
        }
        write!(
            out,
            "  {}  {}",
            order_name(status.order),
            volume_text(status.volume)
        )?;
        if status.stop_after_current {
            write!(out, "  stopping after this track")?;
        }
        write!(out, "{}", style.clear_rest)?;
        out.flush()?;
    }
    Ok(())
}

/// Names the file that has started playing, on a line of its own. Tracks carry no tags yet,
/// so the name is the file's.
pub(crate) fn now_playing(out: &mut impl Write, style: Style, track: &Track) -> io::Result<()> {
    let name = match &track.source {
        Source::File(path) => path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        ),
        other => other.to_string(),
    };
    write!(out, "{name}{}", style.newline)
}

/// Ends the progress line.
pub(crate) fn finish(out: &mut impl Write, style: Style) -> io::Result<()> {
    write!(out, "{}", style.newline)?;
    out.flush()
}

/// `m:ss`, or `h:mm:ss` from an hour.
fn clock(position: Duration) -> String {
    let seconds = position.as_secs();
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use aeri_player::Sequence;

    use super::*;

    fn song(path: &str) -> Arc<Track> {
        Arc::new(Track {
            source: Source::File(PathBuf::from(path)),
            duration: None,
            title: None,
            artist: None,
            album: None,
        })
    }

    fn active(transport: Transport, seconds: u64) -> PlaybackState {
        PlaybackState::Active {
            track: song("a.flac"),
            transport,
            position: Duration::from_secs(seconds),
            output: None,
        }
    }

    fn bare() -> Status {
        Status {
            place: None,
            order: PlaybackOrder::Default,
            volume: Volume::UNITY,
            stop_after_current: false,
        }
    }

    fn shown(write: impl FnOnce(&mut Vec<u8>) -> io::Result<()>) -> String {
        let mut out = Vec::new();
        write(&mut out).expect("writing to memory cannot fail");
        String::from_utf8(out).expect("output is UTF-8")
    }

    #[test]
    fn progress_rewrites_one_line_with_the_transport_the_clock_the_order_and_the_volume() {
        assert_eq!(
            shown(|out| progress(out, Style::PLAIN, &active(Transport::Playing, 61), &bare())),
            "\rplaying   1:01  default  0.0 dB"
        );
        assert_eq!(
            shown(|out| progress(out, Style::PLAIN, &active(Transport::Buffering, 0), &bare())),
            "\rbuffering 0:00  default  0.0 dB"
        );
        assert_eq!(
            shown(|out| progress(out, Style::PLAIN, &active(Transport::Paused, 5), &bare())),
            "\rpaused    0:05  default  0.0 dB"
        );
    }

    #[test]
    fn progress_says_which_entry_of_how_many_when_it_knows() {
        let status = Status {
            place: Some((2, 5)),
            order: PlaybackOrder::Shuffle,
            volume: Volume::from_db(-6.0),
            stop_after_current: false,
        };

        assert_eq!(
            shown(|out| progress(out, Style::PLAIN, &active(Transport::Playing, 1), &status)),
            "\rplaying   0:01  2 of 5  shuffle  -6.0 dB"
        );
    }

    #[test]
    fn progress_says_when_it_will_stop_and_when_the_volume_is_off() {
        let status = Status {
            volume: Volume::SILENT,
            stop_after_current: true,
            ..bare()
        };

        assert_eq!(
            shown(|out| progress(out, Style::PLAIN, &active(Transport::Playing, 1), &status)),
            "\rplaying   0:01  default  muted  stopping after this track"
        );
    }

    #[test]
    fn a_terminal_style_erases_what_a_longer_line_left_behind() {
        assert_eq!(
            shown(|out| progress(
                out,
                Style::RAW_TERMINAL,
                &active(Transport::Playing, 1),
                &bare()
            )),
            "\rplaying   0:01  default  0.0 dB\x1b[K"
        );
    }

    #[test]
    fn every_order_has_its_flag_name() {
        for (order, name) in [
            (PlaybackOrder::Default, "default"),
            (PlaybackOrder::RepeatTrack, "repeat-track"),
            (PlaybackOrder::RepeatPlaylist, "repeat-playlist"),
            (PlaybackOrder::Shuffle, "shuffle"),
        ] {
            assert_eq!(order_name(order), name);
        }
    }

    #[test]
    fn progress_shows_nothing_when_stopped() {
        assert_eq!(
            shown(|out| progress(out, Style::PLAIN, &PlaybackState::Stopped, &bare())),
            ""
        );
    }

    #[test]
    fn the_track_that_started_is_named_by_its_file_on_its_own_line() {
        assert_eq!(
            shown(|out| now_playing(out, Style::PLAIN, &song("/music/album/01 song.flac"))),
            "01 song.flac\n"
        );
        assert_eq!(
            shown(|out| now_playing(out, Style::PLAIN, &song("bare.mp3"))),
            "bare.mp3\n"
        );
    }

    #[test]
    fn a_path_with_no_file_name_is_shown_whole() {
        assert_eq!(
            shown(|out| now_playing(out, Style::PLAIN, &song("/"))),
            "/\n"
        );
    }

    #[test]
    fn a_raw_terminal_gets_a_carriage_return_with_every_new_line() {
        assert_eq!(
            shown(|out| now_playing(out, Style::RAW_TERMINAL, &song("a.flac"))),
            "a.flac\r\n"
        );
        assert_eq!(shown(|out| finish(out, Style::RAW_TERMINAL)), "\r\n");
    }

    #[test]
    fn finish_ends_the_line() {
        assert_eq!(shown(|out| finish(out, Style::PLAIN)), "\n");
    }

    #[test]
    fn short_positions_read_as_minutes_and_seconds() {
        assert_eq!(clock(Duration::ZERO), "0:00");
        assert_eq!(clock(Duration::from_millis(61_900)), "1:01");
        assert_eq!(clock(Duration::from_secs(3_599)), "59:59");
    }

    #[test]
    fn positions_from_an_hour_read_with_hours() {
        assert_eq!(clock(Duration::from_secs(3_600)), "1:00:00");
        assert_eq!(clock(Duration::from_secs(3_600 * 10 + 65)), "10:01:05");
    }

    #[test]
    fn the_place_is_the_entry_counted_from_one_and_the_length_of_its_playlist() {
        let mut sequence = Sequence::new(1);
        let list = sequence.create_playlist("list".to_owned());
        let other = sequence.create_playlist("other".to_owned());
        let ids = sequence
            .append(list, vec![song("a.flac"), song("b.flac"), song("c.flac")])
            .expect("exists");
        let foreign = sequence
            .append(other, vec![song("x.flac")])
            .expect("exists")[0];
        let at = |playlist, entry| Some(Position { playlist, entry });

        assert_eq!(place(sequence.playlists(), at(list, ids[0])), Some((1, 3)));
        assert_eq!(place(sequence.playlists(), at(list, ids[2])), Some((3, 3)));
        assert_eq!(
            place(sequence.playlists(), at(other, foreign)),
            Some((1, 1))
        );
        assert_eq!(place(sequence.playlists(), None), None);
        sequence.remove(list, &[ids[1]]).expect("exists");
        assert_eq!(place(sequence.playlists(), at(list, ids[1])), None);
        assert_eq!(place(sequence.playlists(), at(other, ids[0])), None);
    }
}
