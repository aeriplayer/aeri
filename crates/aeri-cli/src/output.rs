//! Everything `aeri` shows the user, written to whatever the caller passes (stdout in
//! `main`, a buffer in tests). Diagnostics go to stderr through `tracing`.

use std::io::{self, Write};
use std::time::Duration;

use aeri_core::{PlaybackState, Source, Track, Transport};

/// Rewrites the progress line in place.
pub(crate) fn progress(out: &mut impl Write, state: &PlaybackState) -> io::Result<()> {
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
        out.flush()?;
    }
    Ok(())
}

/// Names the file that has started playing, on a line of its own. Tracks carry no tags yet,
/// so the name is the file's.
pub(crate) fn now_playing(out: &mut impl Write, track: &Track) -> io::Result<()> {
    let name = match &track.source {
        Source::File(path) => path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        ),
        other => other.to_string(),
    };
    writeln!(out, "{name}")
}

/// Ends the progress line.
pub(crate) fn finish(out: &mut impl Write) -> io::Result<()> {
    writeln!(out)?;
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

    use aeri_core::{Source, Track};

    use super::*;

    fn active(transport: Transport, seconds: u64) -> PlaybackState {
        PlaybackState::Active {
            track: Arc::new(Track {
                source: Source::File(PathBuf::from("a.flac")),
                duration: None,
                title: None,
                artist: None,
                album: None,
            }),
            transport,
            position: Duration::from_secs(seconds),
            output: None,
        }
    }

    fn shown(write: impl FnOnce(&mut Vec<u8>) -> io::Result<()>) -> String {
        let mut out = Vec::new();
        write(&mut out).expect("writing to memory cannot fail");
        String::from_utf8(out).expect("output is UTF-8")
    }

    #[test]
    fn progress_rewrites_one_line_with_the_transport_and_clock() {
        assert_eq!(
            shown(|out| progress(out, &active(Transport::Playing, 61))),
            "\rplaying   1:01"
        );
        assert_eq!(
            shown(|out| progress(out, &active(Transport::Buffering, 0))),
            "\rbuffering 0:00"
        );
        assert_eq!(
            shown(|out| progress(out, &active(Transport::Paused, 5))),
            "\rpaused    0:05"
        );
    }

    #[test]
    fn progress_shows_nothing_when_stopped() {
        assert_eq!(shown(|out| progress(out, &PlaybackState::Stopped)), "");
    }

    fn file(path: &str) -> Track {
        Track {
            source: Source::File(PathBuf::from(path)),
            duration: None,
            title: None,
            artist: None,
            album: None,
        }
    }

    #[test]
    fn the_track_that_started_is_named_by_its_file_on_its_own_line() {
        assert_eq!(
            shown(|out| now_playing(out, &file("/music/album/01 song.flac"))),
            "01 song.flac\n"
        );
        assert_eq!(
            shown(|out| now_playing(out, &file("bare.mp3"))),
            "bare.mp3\n"
        );
    }

    #[test]
    fn a_path_with_no_file_name_is_shown_whole() {
        assert_eq!(shown(|out| now_playing(out, &file("/"))), "/\n");
    }

    #[test]
    fn finish_ends_the_line() {
        assert_eq!(shown(finish), "\n");
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
}
