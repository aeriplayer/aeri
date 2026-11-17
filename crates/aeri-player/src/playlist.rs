//! An ordered list of entries.

use std::sync::Arc;

use aeri_core::Track;

use crate::ids::{EntryId, PlaylistId};

/// One place in a playlist. The same track may have several entries.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Entry {
    /// Names this entry for as long as the process runs.
    pub id: EntryId,
    /// What plays.
    pub track: Arc<Track>,
}

/// An ordered list of entries, which a frontend reads and only the sequence edits.
#[derive(Debug, Clone)]
pub struct Playlist {
    id: PlaylistId,
    name: String,
    entries: Vec<Entry>,
}

impl Playlist {
    pub(crate) const fn new(id: PlaylistId, name: String) -> Self {
        Self {
            id,
            name,
            entries: Vec::new(),
        }
    }

    /// The playlist's id.
    #[must_use]
    pub const fn id(&self) -> PlaylistId {
        self.id
    }

    /// The name it was created with.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The entries, in order.
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// How many entries there are.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there are none.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The entry with this id, if it is still here.
    #[must_use]
    pub fn get(&self, id: EntryId) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    /// Whether the entry is still here.
    #[must_use]
    pub fn contains(&self, id: EntryId) -> bool {
        self.get(id).is_some()
    }

    pub(crate) fn position(&self, id: EntryId) -> Option<usize> {
        self.entries.iter().position(|entry| entry.id == id)
    }

    /// Inserts `entry` before the entry now at `at`, or at the end when `at` is past it.
    pub(crate) fn insert(&mut self, at: usize, entry: Entry) {
        self.entries.insert(at.min(self.entries.len()), entry);
    }

    pub(crate) fn remove(&mut self, id: EntryId) -> Option<Entry> {
        let at = self.position(id)?;
        Some(self.entries.remove(at))
    }

    /// Moves `id` to just before `before`, or to the end when `before` is `None`. Does
    /// nothing, and says so, when either id is not here.
    pub(crate) fn move_before(&mut self, id: EntryId, before: Option<EntryId>) -> bool {
        let Some(from) = self.position(id) else {
            return false;
        };
        if before == Some(id) {
            return true;
        }
        if before.is_some_and(|before| self.position(before).is_none()) {
            return false;
        }
        let entry = self.entries.remove(from);
        let at = before
            .and_then(|before| self.position(before))
            .unwrap_or(self.entries.len());
        self.entries.insert(at, entry);
        true
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use aeri_core::Source;

    use super::*;
    use crate::ids::Ids;

    fn track(title: &str) -> Arc<Track> {
        Arc::new(Track {
            source: Source::File(format!("{title}.flac").into()),
            duration: None,
            title: Some(title.to_owned()),
            artist: None,
            album: None,
        })
    }

    fn playlist(titles: &[&str]) -> (Playlist, Vec<EntryId>) {
        let mut ids = Ids::new();
        let mut playlist = Playlist::new(ids.next_playlist(), "list".to_owned());
        let mut added = Vec::new();
        for title in titles {
            let id = ids.next_entry();
            playlist.insert(
                playlist.len(),
                Entry {
                    id,
                    track: track(title),
                },
            );
            added.push(id);
        }
        (playlist, added)
    }

    fn order(playlist: &Playlist) -> Vec<EntryId> {
        playlist.entries().iter().map(|entry| entry.id).collect()
    }

    #[test]
    fn a_new_playlist_is_empty_and_keeps_its_name() {
        let (playlist, _) = playlist(&[]);

        assert!(playlist.is_empty());
        assert_eq!(playlist.len(), 0);
        assert_eq!(playlist.name(), "list");
        assert_eq!(playlist.id().get(), 1);
    }

    #[test]
    fn a_playlist_with_entries_is_not_empty() {
        let (playlist, ids) = playlist(&["a", "b"]);

        assert!(!playlist.is_empty());
        assert_eq!(playlist.len(), ids.len());
    }

    #[test]
    fn insert_places_before_the_given_position_and_clamps_past_the_end() {
        let (mut playlist, ids) = playlist(&["a", "b"]);
        let mut more = Ids::new();
        let _ = (more.next_entry(), more.next_entry());
        let (x, y) = (more.next_entry(), more.next_entry());

        playlist.insert(
            1,
            Entry {
                id: x,
                track: track("x"),
            },
        );
        playlist.insert(
            99,
            Entry {
                id: y,
                track: track("y"),
            },
        );

        assert_eq!(order(&playlist), [ids[0], x, ids[1], y]);
    }

    #[test]
    fn the_same_track_can_have_two_entries() {
        let (mut playlist, ids) = playlist(&["a"]);
        let again = Arc::clone(&playlist.get(ids[0]).expect("present").track);
        let mut more = Ids::new();
        let _ = more.next_entry();
        let second = more.next_entry();

        playlist.insert(
            1,
            Entry {
                id: second,
                track: again,
            },
        );

        assert_eq!(playlist.len(), 2);
        assert!(Arc::ptr_eq(
            &playlist.entries()[0].track,
            &playlist.entries()[1].track
        ));
        assert_ne!(playlist.entries()[0].id, playlist.entries()[1].id);
    }

    #[test]
    fn remove_takes_out_one_entry_by_id_and_reports_an_unknown_one() {
        let (mut playlist, ids) = playlist(&["a", "b", "c"]);

        assert_eq!(playlist.remove(ids[1]).map(|entry| entry.id), Some(ids[1]));
        assert_eq!(order(&playlist), [ids[0], ids[2]]);
        assert!(playlist.remove(ids[1]).is_none());
        assert!(!playlist.contains(ids[1]));
        assert!(playlist.contains(ids[0]));
    }

    #[test]
    fn get_finds_an_entry_by_id() {
        let (playlist, ids) = playlist(&["a", "b"]);

        assert_eq!(
            playlist
                .get(ids[1])
                .and_then(|entry| entry.track.title.as_deref()),
            Some("b")
        );
        assert_eq!(playlist.position(ids[1]), Some(1));
    }

    #[test]
    fn move_before_places_an_entry_ahead_of_another_in_either_direction() {
        let (mut playlist, ids) = playlist(&["a", "b", "c", "d"]);

        assert!(playlist.move_before(ids[3], Some(ids[1])));
        assert_eq!(order(&playlist), [ids[0], ids[3], ids[1], ids[2]]);

        assert!(playlist.move_before(ids[0], Some(ids[2])));
        assert_eq!(order(&playlist), [ids[3], ids[1], ids[0], ids[2]]);
    }

    #[test]
    fn move_before_none_moves_to_the_end() {
        let (mut playlist, ids) = playlist(&["a", "b", "c"]);

        assert!(playlist.move_before(ids[0], None));

        assert_eq!(order(&playlist), [ids[1], ids[2], ids[0]]);
    }

    #[test]
    fn moving_an_entry_before_itself_changes_nothing() {
        let (mut playlist, ids) = playlist(&["a", "b", "c"]);

        assert!(playlist.move_before(ids[1], Some(ids[1])));

        assert_eq!(order(&playlist), ids);
    }

    #[test]
    fn move_before_with_an_unknown_id_changes_nothing_and_says_so() {
        let (mut playlist, ids) = playlist(&["a", "b"]);
        let mut other = Ids::new();
        let stranger = (0..5).map(|_| other.next_entry()).last().expect("five ids");

        assert!(!playlist.move_before(stranger, Some(ids[0])));
        assert!(!playlist.move_before(ids[0], Some(stranger)));
        assert!(!playlist.move_before(stranger, None));

        assert_eq!(order(&playlist), ids);
    }

    #[test]
    fn clear_empties_the_playlist() {
        let (mut playlist, _) = playlist(&["a", "b"]);

        playlist.clear();

        assert!(playlist.is_empty());
    }
}
