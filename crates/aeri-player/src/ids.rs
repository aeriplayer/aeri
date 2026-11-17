//! Ids that name playlists and entries, handed out by counters and never reused.

use std::fmt;
use std::num::NonZeroU64;

/// Names a playlist. An index names a position, and positions move under every edit; an id
/// names the playlist itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PlaylistId(NonZeroU64);

/// Names one entry of a playlist, so the same track can appear twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntryId(NonZeroU64);

impl PlaylistId {
    /// The id as a number, for display.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

impl EntryId {
    /// The id as a number, for display.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

impl fmt::Display for PlaylistId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::Display for EntryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// An entry, named by the playlist it is in and its own id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Position {
    /// The playlist the entry is in.
    pub playlist: PlaylistId,
    /// The entry.
    pub entry: EntryId,
}

/// The two counters. Starting at one keeps every id non-zero.
#[derive(Debug)]
pub(crate) struct Ids {
    playlist: NonZeroU64,
    entry: NonZeroU64,
}

impl Ids {
    pub(crate) const fn new() -> Self {
        Self {
            playlist: NonZeroU64::MIN,
            entry: NonZeroU64::MIN,
        }
    }

    pub(crate) fn next_playlist(&mut self) -> PlaylistId {
        PlaylistId(bump(&mut self.playlist))
    }

    pub(crate) fn next_entry(&mut self) -> EntryId {
        EntryId(bump(&mut self.entry))
    }
}

fn bump(counter: &mut NonZeroU64) -> NonZeroU64 {
    let id = *counter;
    *counter = id
        .checked_add(1)
        .expect("a u64 counter that advances once per edit cannot run out");
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_count_up_from_one_and_are_not_shared_between_kinds() {
        let mut ids = Ids::new();

        assert_eq!(ids.next_playlist().get(), 1);
        assert_eq!(ids.next_playlist().get(), 2);
        assert_eq!(ids.next_entry().get(), 1);
        assert_eq!(ids.next_entry().get(), 2);
        assert_eq!(ids.next_playlist().get(), 3);
    }

    #[test]
    fn ids_display_as_their_number() {
        let mut ids = Ids::new();
        let _ = ids.next_entry();

        assert_eq!(ids.next_entry().to_string(), "2");
        assert_eq!(ids.next_playlist().to_string(), "1");
    }

    #[test]
    fn an_optional_id_is_no_larger_than_the_id() {
        assert_eq!(size_of::<Option<EntryId>>(), size_of::<EntryId>());
    }
}
