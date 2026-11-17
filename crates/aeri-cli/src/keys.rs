//! The keyboard: which key does what, and reading keys from a terminal in raw mode.

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal;
use tokio::sync::mpsc;

/// What a key asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    TogglePause,
    Next,
    Previous,
    SeekForward,
    SeekBack,
    VolumeUp,
    VolumeDown,
    CycleOrder,
    ToggleStopAfterCurrent,
    Quit,
}

/// The action a key press stands for, if any. Key releases and chords with Ctrl or Alt are
/// not actions, except Ctrl-C: in raw mode the terminal delivers it as a key, not a signal.
pub(crate) fn action(key: KeyEvent) -> Option<Action> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return (key.code == KeyCode::Char('c')).then_some(Action::Quit);
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        return None;
    }
    match key.code {
        KeyCode::Char(' ') => Some(Action::TogglePause),
        KeyCode::Char('n') => Some(Action::Next),
        KeyCode::Char('p') => Some(Action::Previous),
        KeyCode::Right => Some(Action::SeekForward),
        KeyCode::Left => Some(Action::SeekBack),
        KeyCode::Up => Some(Action::VolumeUp),
        KeyCode::Down => Some(Action::VolumeDown),
        KeyCode::Char('o') => Some(Action::CycleOrder),
        KeyCode::Char('s') => Some(Action::ToggleStopAfterCurrent),
        KeyCode::Char('q') => Some(Action::Quit),
        _ => None,
    }
}

/// How often the reader looks up to see whether it should stop.
const LOOK_UP_EVERY: Duration = Duration::from_millis(100);

/// Waits up to a timeout for a key.
type Source = dyn FnMut(Duration) -> io::Result<Option<KeyEvent>> + Send;

/// The terminal in raw mode, and a thread reading its keys. Dropping this stops the thread and
/// restores the terminal, however the scope is left: the reader is dropped first, then the mode.
#[derive(Debug)]
pub(crate) struct Keys {
    _reader: Reader,
    _raw: RawMode,
}

/// Puts the terminal back as it was when dropped.
#[derive(Debug)]
struct RawMode;

impl RawMode {
    fn enable() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        if let Err(error) = terminal::disable_raw_mode() {
            tracing::warn!(%error, "could not restore the terminal");
        }
    }
}

impl Keys {
    /// Switches the terminal to raw mode and starts reading keys, which arrive on the receiver.
    pub(crate) fn start() -> io::Result<(Self, mpsc::UnboundedReceiver<KeyEvent>)> {
        let raw = RawMode::enable()?;
        let (reader, received) = Reader::spawn(Box::new(from_terminal))?;
        Ok((
            Self {
                _reader: reader,
                _raw: raw,
            },
            received,
        ))
    }
}

/// The next key the terminal gives within `timeout`. Anything else it reports is skipped.
fn from_terminal(timeout: Duration) -> io::Result<Option<KeyEvent>> {
    if event::poll(timeout)?
        && let Event::Key(key) = event::read()?
    {
        return Ok(Some(key));
    }
    Ok(None)
}

/// A thread that reads keys from a source and forwards them. Dropping it stops and joins it.
#[derive(Debug)]
struct Reader {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Reader {
    fn spawn(source: Box<Source>) -> io::Result<(Self, mpsc::UnboundedReceiver<KeyEvent>)> {
        let (keys, received) = mpsc::unbounded_channel();
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let stop = Arc::clone(&stop);
            thread::Builder::new()
                .name("aeri-keys".into())
                .spawn(move || read_keys(&stop, &keys, source))?
        };
        Ok((
            Self {
                stop,
                thread: Some(thread),
            },
            received,
        ))
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::error!("the key reader panicked");
        }
    }
}

fn read_keys(stop: &AtomicBool, keys: &mpsc::UnboundedSender<KeyEvent>, mut source: Box<Source>) {
    while !stop.load(Ordering::Relaxed) {
        match source(LOOK_UP_EVERY) {
            Ok(Some(key)) => {
                if keys.send(key).is_err() {
                    break;
                }
            }
            Ok(None) => {}
            Err(error) => {
                tracing::debug!(%error, "the terminal stopped giving keys");
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn chord(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn each_key_of_the_brief_stands_for_its_action() {
        for (code, expected) in [
            (KeyCode::Char(' '), Action::TogglePause),
            (KeyCode::Char('n'), Action::Next),
            (KeyCode::Char('p'), Action::Previous),
            (KeyCode::Right, Action::SeekForward),
            (KeyCode::Left, Action::SeekBack),
            (KeyCode::Up, Action::VolumeUp),
            (KeyCode::Down, Action::VolumeDown),
            (KeyCode::Char('o'), Action::CycleOrder),
            (KeyCode::Char('s'), Action::ToggleStopAfterCurrent),
            (KeyCode::Char('q'), Action::Quit),
        ] {
            assert_eq!(action(press(code)), Some(expected), "{code:?}");
        }
    }

    #[test]
    fn control_c_quits_because_raw_mode_delivers_it_as_a_key() {
        assert_eq!(
            action(chord(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(Action::Quit)
        );
    }

    #[test]
    fn other_control_and_alt_chords_do_nothing() {
        for modifiers in [
            KeyModifiers::CONTROL,
            KeyModifiers::ALT,
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        ] {
            for code in [KeyCode::Char('n'), KeyCode::Char('q'), KeyCode::Right] {
                assert_eq!(
                    action(chord(code, modifiers)),
                    None,
                    "{code:?} {modifiers:?}"
                );
            }
        }
        assert_eq!(
            action(chord(KeyCode::Char('x'), KeyModifiers::CONTROL)),
            None
        );
    }

    #[test]
    fn a_shifted_letter_is_not_the_lowercase_command() {
        assert_eq!(action(chord(KeyCode::Char('N'), KeyModifiers::SHIFT)), None);
        assert_eq!(action(chord(KeyCode::Char('Q'), KeyModifiers::SHIFT)), None);
    }

    #[test]
    fn unmapped_keys_do_nothing() {
        for code in [
            KeyCode::Char('x'),
            KeyCode::Enter,
            KeyCode::Esc,
            KeyCode::Tab,
            KeyCode::F(1),
        ] {
            assert_eq!(action(press(code)), None, "{code:?}");
        }
    }

    #[test]
    fn a_release_is_not_an_action_but_a_repeat_is() {
        let release = KeyEvent::new_with_kind(
            KeyCode::Char('n'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        );
        let repeat =
            KeyEvent::new_with_kind(KeyCode::Down, KeyModifiers::NONE, KeyEventKind::Repeat);

        assert_eq!(action(release), None);
        assert_eq!(action(repeat), Some(Action::VolumeDown));
    }

    // ---- the reader, with a source made by hand ----

    use std::collections::VecDeque;

    fn idle() {
        thread::sleep(Duration::from_millis(2));
    }

    #[test]
    fn the_reader_forwards_keys_in_order_and_keeps_waiting_for_more() {
        let mut script: VecDeque<KeyEvent> = [KeyCode::Char('n'), KeyCode::Up, KeyCode::Char('q')]
            .into_iter()
            .map(press)
            .collect();
        let (reader, mut received) = Reader::spawn(Box::new(move |_| {
            let next = script.pop_front();
            if next.is_none() {
                idle();
            }
            Ok(next)
        }))
        .expect("the thread starts");

        let keys: Vec<_> = (0..3)
            .map(|_| received.blocking_recv().expect("a key arrives").code)
            .collect();

        assert_eq!(keys, [KeyCode::Char('n'), KeyCode::Up, KeyCode::Char('q')]);
        drop(reader);
    }

    #[test]
    fn dropping_the_reader_stops_and_joins_its_thread() {
        let alive = Arc::new(());
        let held = Arc::clone(&alive);
        let (reader, _received) = Reader::spawn(Box::new(move |_| {
            let _ = &held;
            idle();
            Ok(None)
        }))
        .expect("the thread starts");
        assert_eq!(Arc::strong_count(&alive), 2);

        drop(reader);

        assert_eq!(
            Arc::strong_count(&alive),
            1,
            "the thread still holds its source"
        );
    }

    #[test]
    fn a_terminal_that_fails_ends_the_reader() {
        let (reader, _received) = Reader::spawn(Box::new(|_| {
            Err(io::Error::other("the terminal went away"))
        }))
        .expect("the thread starts");

        let thread = reader.thread.as_ref().expect("running");
        for _ in 0..1_000 {
            if thread.is_finished() {
                return;
            }
            idle();
        }
        panic!("the reader kept going after the terminal failed");
    }

    #[test]
    fn a_reader_whose_receiver_is_gone_ends() {
        let (reader, received) = Reader::spawn(Box::new(|_| Ok(Some(press(KeyCode::Char('n'))))))
            .expect("the thread starts");
        drop(received);

        let thread = reader.thread.as_ref().expect("running");
        for _ in 0..1_000 {
            if thread.is_finished() {
                return;
            }
            idle();
        }
        panic!("the reader kept going with nobody listening");
    }
}
