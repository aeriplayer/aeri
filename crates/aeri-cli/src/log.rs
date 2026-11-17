//! Log lines that do not tear the progress line.
//!
//! The progress line is rewritten in place with no newline, so a warning printed after it lands
//! in the middle of it. Here a line first returns to the start of the line and erases it, and in
//! raw mode, which turns off the terminal's newline translation, ends with a carriage return too.

use std::io::{self, IsTerminal as _, Write};
use std::sync::atomic::{AtomicU8, Ordering};

use tracing_subscriber::fmt::MakeWriter;

/// What the terminal looks like to a log line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Screen {
    /// Nothing is being drawn: log as plain lines.
    Quiet,
    /// A progress line is being rewritten in place.
    ProgressLine,
    /// As above, and the terminal is in raw mode.
    RawTerminal,
}

impl Screen {
    const fn code(self) -> u8 {
        match self {
            Self::Quiet => 0,
            Self::ProgressLine => 1,
            Self::RawTerminal => 2,
        }
    }

    const fn from_code(code: u8) -> Self {
        match code {
            1 => Self::ProgressLine,
            2 => Self::RawTerminal,
            _ => Self::Quiet,
        }
    }
}

static SCREEN: AtomicU8 = AtomicU8::new(0);

/// Says what is being drawn from now on.
pub(crate) fn set_screen(screen: Screen) {
    SCREEN.store(screen.code(), Ordering::Relaxed);
}

/// Returns to the start of the line and erases it.
const RESTART_LINE: &[u8] = b"\r\x1b[K";

/// What to write for `text`, the first piece of a log line or a later one.
pub(crate) fn frame(screen: Screen, first: bool, text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() + RESTART_LINE.len());
    if first && screen != Screen::Quiet {
        out.extend_from_slice(RESTART_LINE);
    }
    let mut previous = 0_u8;
    for &byte in text {
        if byte == b'\n' && screen == Screen::RawTerminal && previous != b'\r' {
            out.push(b'\r');
        }
        out.push(byte);
        previous = byte;
    }
    out
}

/// The screen a log line sees: what was set, unless the output is not a terminal.
fn screen_seen(is_terminal: bool) -> Screen {
    if is_terminal {
        Screen::from_code(SCREEN.load(Ordering::Relaxed))
    } else {
        Screen::Quiet
    }
}

/// Makes a writer to standard error for each log line.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LogWriter;

impl MakeWriter<'_> for LogWriter {
    type Writer = Line<io::Stderr>;

    fn make_writer(&self) -> Self::Writer {
        Line::new(io::stderr(), screen_seen(io::stderr().is_terminal()))
    }
}

/// One log line on its way to `out`.
#[derive(Debug)]
pub(crate) struct Line<W> {
    out: W,
    screen: Screen,
    started: bool,
}

impl<W: Write> Line<W> {
    fn new(out: W, screen: Screen) -> Self {
        Self {
            out,
            screen,
            started: false,
        }
    }
}

impl<W: Write> Write for Line<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let framed = frame(self.screen, !self.started, buf);
        self.started = true;
        self.out.write_all(&framed)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(bytes: &[u8]) -> String {
        String::from_utf8(bytes.to_vec()).expect("UTF-8")
    }

    #[test]
    fn a_quiet_screen_passes_a_line_through_untouched() {
        assert_eq!(text(&frame(Screen::Quiet, true, b"WARN x\n")), "WARN x\n");
    }

    #[test]
    fn a_progress_line_is_erased_before_the_first_piece_of_a_log_line_only() {
        assert_eq!(
            text(&frame(Screen::ProgressLine, true, b"WARN x\n")),
            "\r\x1b[KWARN x\n"
        );
        assert_eq!(
            text(&frame(Screen::ProgressLine, false, b"more\n")),
            "more\n"
        );
    }

    #[test]
    fn raw_mode_ends_lines_with_a_carriage_return_and_never_doubles_one() {
        assert_eq!(
            text(&frame(Screen::RawTerminal, true, b"a\nb\r\nc\n")),
            "\r\x1b[Ka\r\nb\r\nc\r\n"
        );
        assert_eq!(text(&frame(Screen::RawTerminal, false, b"\n")), "\r\n");
    }

    #[test]
    fn the_screen_survives_a_round_trip_through_its_code() {
        for screen in [Screen::Quiet, Screen::ProgressLine, Screen::RawTerminal] {
            assert_eq!(Screen::from_code(screen.code()), screen);
        }
        assert_eq!(Screen::from_code(200), Screen::Quiet);
    }

    #[test]
    fn a_line_is_framed_once_at_its_start_and_reports_the_bytes_it_was_given() {
        let mut line = Line::new(Vec::new(), Screen::RawTerminal);

        assert_eq!(line.write(b"WARN a\n").expect("writes"), 7);
        assert_eq!(line.write(b"b\n").expect("writes"), 2);
        line.flush().expect("flushes");

        assert_eq!(text(&line.out), "\r\x1b[KWARN a\r\nb\r\n");
    }

    #[test]
    fn flushing_a_line_flushes_its_output() {
        #[derive(Default)]
        struct Watching {
            flushed: bool,
        }

        impl Write for Watching {
            fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                Ok(buf.len())
            }

            fn flush(&mut self) -> io::Result<()> {
                self.flushed = true;
                Ok(())
            }
        }

        let mut line = Line::new(Watching::default(), Screen::Quiet);
        line.flush().expect("flushes");

        assert!(line.out.flushed);
    }

    #[test]
    fn a_quiet_line_is_written_as_it_came() {
        let mut line = Line::new(Vec::new(), Screen::Quiet);

        line.write_all(b"plain\n").expect("writes");

        assert_eq!(text(&line.out), "plain\n");
    }

    #[test]
    fn the_screen_that_was_set_is_seen_only_on_a_terminal() {
        set_screen(Screen::RawTerminal);

        assert_eq!(screen_seen(true), Screen::RawTerminal);
        assert_eq!(screen_seen(false), Screen::Quiet);
        set_screen(Screen::ProgressLine);
        assert_eq!(screen_seen(true), Screen::ProgressLine);
    }
}
