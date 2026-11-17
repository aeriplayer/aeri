//! What plays next, decided in one place from plain data (ADR 0024).

use std::collections::VecDeque;
use std::sync::Arc;

use aeri_core::Track;
use fastrand::Rng;

use crate::error::SequenceError;
use crate::ids::{EntryId, Ids, PlaylistId, Position};
use crate::playlist::{Entry, Playlist};
use crate::shuffle::Shuffle;

#[cfg(test)]
mod tests;

/// The order entries play in when nothing is queued.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PlaybackOrder {
    /// Each entry once, in order, then stop.
    #[default]
    Default,
    /// The same entry again, until the user skips.
    RepeatTrack,
    /// Each entry in order, from the first again after the last.
    RepeatPlaylist,
    /// Every entry once per pass, in a random order, then a new pass.
    Shuffle,
}

/// The playlists, the queue, and everything that decides what plays after what.
///
/// The `peek_*` queries do not change anything and answer the same until an edit or a
/// [`take`](Self::take). The actor above calls them and decides nothing itself.
#[derive(Debug)]
pub struct Sequence {
    ids: Ids,
    playlists: Vec<Arc<Playlist>>,
    active: Option<PlaylistId>,
    /// The last entry taken from the order, in the active playlist. Taking a queued entry
    /// leaves it alone, so the order resumes where it was.
    cursor: Option<EntryId>,
    /// The last entry taken from anywhere.
    current: Option<Position>,
    queue: VecDeque<Position>,
    order: PlaybackOrder,
    stop_after_current: bool,
    /// Present exactly when the order is shuffle and there is an active playlist.
    shuffle: Option<Shuffle>,
    rng: Rng,
}

fn find(playlists: &[Arc<Playlist>], id: PlaylistId) -> Option<&Arc<Playlist>> {
    playlists.iter().find(|playlist| playlist.id() == id)
}

impl Sequence {
    /// An empty sequence. `seed` makes shuffle repeatable.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self {
            ids: Ids::new(),
            playlists: Vec::new(),
            active: None,
            cursor: None,
            current: None,
            queue: VecDeque::new(),
            order: PlaybackOrder::Default,
            stop_after_current: false,
            shuffle: None,
            rng: Rng::with_seed(seed),
        }
    }

    /// Every playlist, in the order they were created.
    #[must_use]
    pub fn playlists(&self) -> &[Arc<Playlist>] {
        &self.playlists
    }

    /// The playlist with this id.
    #[must_use]
    pub fn playlist(&self, id: PlaylistId) -> Option<&Arc<Playlist>> {
        find(&self.playlists, id)
    }

    /// The playlist the order is read from.
    #[must_use]
    pub const fn active(&self) -> Option<PlaylistId> {
        self.active
    }

    /// The last entry taken, queued or not.
    #[must_use]
    pub const fn current(&self) -> Option<Position> {
        self.current
    }

    /// The queued entries, first to play first.
    pub fn queue(&self) -> impl ExactSizeIterator<Item = Position> + '_ {
        self.queue.iter().copied()
    }

    /// The playback order.
    #[must_use]
    pub const fn order(&self) -> PlaybackOrder {
        self.order
    }

    /// Whether playback stops when the current entry ends.
    #[must_use]
    pub const fn stop_after_current(&self) -> bool {
        self.stop_after_current
    }

    fn require(&self, playlist: PlaylistId) -> Result<&Arc<Playlist>, SequenceError> {
        find(&self.playlists, playlist).ok_or(SequenceError::UnknownPlaylist(playlist))
    }

    fn require_entry(&self, position: Position) -> Result<(), SequenceError> {
        if self.require(position.playlist)?.contains(position.entry) {
            Ok(())
        } else {
            Err(SequenceError::UnknownEntry(position.entry))
        }
    }

    // ---- queries ----

    /// What plays when the current entry ends: nothing when stop-after-current is set, else
    /// the queue's front, else the order's choice.
    #[must_use]
    pub fn peek_next(&self) -> Option<Position> {
        if self.stop_after_current {
            return None;
        }
        self.queue
            .front()
            .copied()
            .or_else(|| self.ordered_next(false))
    }

    /// What plays when the user skips forward. Stop-after-current and repeat-track do not
    /// apply to a skip.
    #[must_use]
    pub fn peek_skip_next(&self) -> Option<Position> {
        self.queue
            .front()
            .copied()
            .or_else(|| self.ordered_next(true))
    }

    fn ordered_next(&self, skip: bool) -> Option<Position> {
        let list = find(&self.playlists, self.active?)?;
        let entries = list.entries();
        let at = self.cursor.and_then(|cursor| list.position(cursor));
        let entry = match self.order {
            PlaybackOrder::Default => entries.get(at.map_or(0, |at| at + 1))?.id,
            PlaybackOrder::RepeatTrack if skip => entries.get(at.map_or(0, |at| at + 1))?.id,
            PlaybackOrder::RepeatTrack => entries.get(at.unwrap_or(0))?.id,
            PlaybackOrder::RepeatPlaylist => {
                entries.get(at.map_or(0, |at| (at + 1) % entries.len()))?.id
            }
            PlaybackOrder::Shuffle => self.shuffle.as_ref()?.peek()?,
        };
        Some(Position {
            playlist: list.id(),
            entry,
        })
    }

    /// The entry before the cursor, never the cursor itself. When the current entry came from
    /// the queue, that is the cursor's entry.
    #[must_use]
    pub fn peek_previous(&self) -> Option<Position> {
        let list = find(&self.playlists, self.active?)?;
        let cursor = self.cursor?;
        let here = Position {
            playlist: list.id(),
            entry: cursor,
        };
        if self.current.is_some_and(|current| current != here) {
            return Some(here);
        }
        let entries = list.entries();
        let at = list.position(cursor)?;
        let entry = match self.order {
            PlaybackOrder::Default | PlaybackOrder::RepeatTrack => {
                entries.get(at.checked_sub(1)?)?.id
            }
            PlaybackOrder::RepeatPlaylist => {
                entries
                    .get(at.checked_sub(1).or_else(|| entries.len().checked_sub(1))?)?
                    .id
            }
            PlaybackOrder::Shuffle => self.shuffle.as_ref()?.peek_previous()?,
        };
        Some(Position {
            playlist: list.id(),
            entry,
        })
    }

    // ---- taking an entry ----

    /// The engine took `position` from the slot, or the user skipped to it: it is now the
    /// current entry. A queued entry leaves the queue and the cursor where it was; any other
    /// becomes the cursor. An entry that is no longer there is ignored.
    pub fn take(&mut self, position: Position) {
        if self.require_entry(position).is_err() {
            return;
        }
        self.current = Some(position);
        if self.queue.front() == Some(&position) {
            self.queue.pop_front();
            return;
        }
        self.enter(position);
    }

    /// Plays `position` now, from the middle of the order.
    ///
    /// # Errors
    ///
    /// [`SequenceError`] when the playlist or entry does not exist.
    pub fn jump_to(&mut self, position: Position) -> Result<(), SequenceError> {
        self.require_entry(position)?;
        self.current = Some(position);
        self.enter(position);
        Ok(())
    }

    fn enter(&mut self, position: Position) {
        if self.active != Some(position.playlist) {
            self.active = Some(position.playlist);
            self.cursor = None;
            self.rebuild_shuffle();
        }
        self.cursor = Some(position.entry);
        if let Some(shuffle) = &mut self.shuffle {
            shuffle.take(position.entry, &mut self.rng);
        }
    }

    /// Takes whatever [`peek_next`](Self::peek_next) names. When nothing plays next, clears
    /// stop-after-current, which has now taken effect.
    pub fn advance(&mut self) -> Option<Position> {
        let next = self.peek_next();
        match next {
            Some(position) => self.take(position),
            None => self.stopped(),
        }
        next
    }

    /// Playback stopped on its own. Stop-after-current has done its work.
    pub fn stopped(&mut self) {
        self.stop_after_current = false;
    }

    // ---- edits ----

    /// Creates an empty playlist. The first one created becomes the active playlist.
    pub fn create_playlist(&mut self, name: String) -> PlaylistId {
        let id = self.ids.next_playlist();
        self.playlists.push(Arc::new(Playlist::new(id, name)));
        if self.active.is_none() {
            self.active = Some(id);
            self.rebuild_shuffle();
        }
        id
    }

    /// Adds an entry for each track, at the end, and returns their ids in order.
    ///
    /// # Errors
    ///
    /// [`SequenceError::UnknownPlaylist`] when `to` does not exist.
    pub fn append(
        &mut self,
        to: PlaylistId,
        tracks: Vec<Arc<Track>>,
    ) -> Result<Vec<EntryId>, SequenceError> {
        self.require(to)?;
        let Some(list) = self
            .playlists
            .iter_mut()
            .find(|playlist| playlist.id() == to)
            .map(Arc::make_mut)
        else {
            return Err(SequenceError::UnknownPlaylist(to));
        };
        let mut added = Vec::with_capacity(tracks.len());
        for track in tracks {
            let id = self.ids.next_entry();
            list.insert(list.len(), Entry { id, track });
            added.push(id);
        }
        if self.active == Some(to)
            && let Some(shuffle) = &mut self.shuffle
        {
            for &id in &added {
                shuffle.add(id, &mut self.rng);
            }
        }
        Ok(added)
    }

    /// Removes the entries from the playlist and from the queue. Removing the entry the order
    /// is at lets it carry on with that entry's successor.
    ///
    /// # Errors
    ///
    /// [`SequenceError`] when the playlist or any entry does not exist; nothing is removed.
    pub fn remove(&mut self, from: PlaylistId, entries: &[EntryId]) -> Result<(), SequenceError> {
        let list = self.require(from)?;
        if let Some(&unknown) = entries.iter().find(|&&entry| !list.contains(entry)) {
            return Err(SequenceError::UnknownEntry(unknown));
        }
        for &entry in entries {
            self.remove_one(from, entry);
        }
        Ok(())
    }

    fn remove_one(&mut self, from: PlaylistId, entry: EntryId) {
        let Some(list) = self
            .playlists
            .iter_mut()
            .find(|playlist| playlist.id() == from)
        else {
            return;
        };
        let before = list.position(entry).and_then(|at| at.checked_sub(1));
        let predecessor = before.and_then(|at| list.entries().get(at)).map(|e| e.id);
        if Arc::make_mut(list).remove(entry).is_none() {
            return;
        }
        let position = Position {
            playlist: from,
            entry,
        };
        self.queue.retain(|queued| *queued != position);
        if self.active != Some(from) {
            return;
        }
        if let Some(shuffle) = &mut self.shuffle {
            shuffle.remove(entry);
        }
        if self.cursor == Some(entry) {
            self.cursor = self
                .shuffle
                .as_ref()
                .map_or(predecessor, Shuffle::last_taken);
        }
    }

    /// Removes every entry from the playlist and from the queue.
    ///
    /// # Errors
    ///
    /// [`SequenceError::UnknownPlaylist`] when it does not exist.
    pub fn clear(&mut self, playlist: PlaylistId) -> Result<(), SequenceError> {
        let all: Vec<EntryId> = self
            .require(playlist)?
            .entries()
            .iter()
            .map(|entry| entry.id)
            .collect();
        self.remove(playlist, &all)?;
        if let Some(list) = self
            .playlists
            .iter_mut()
            .find(|candidate| candidate.id() == playlist)
        {
            Arc::make_mut(list).clear();
        }
        Ok(())
    }

    /// Moves `entry` to just before `before`, or to the end when `before` is `None`.
    ///
    /// # Errors
    ///
    /// [`SequenceError`] when the playlist, the entry, or `before` does not exist.
    pub fn move_before(
        &mut self,
        playlist: PlaylistId,
        entry: EntryId,
        before: Option<EntryId>,
    ) -> Result<(), SequenceError> {
        let list = self.require(playlist)?;
        for id in std::iter::once(entry).chain(before) {
            if !list.contains(id) {
                return Err(SequenceError::UnknownEntry(id));
            }
        }
        if let Some(list) = self
            .playlists
            .iter_mut()
            .find(|candidate| candidate.id() == playlist)
        {
            Arc::make_mut(list).move_before(entry, before);
        }
        Ok(())
    }

    /// Makes `playlist` the one the order is read from, starting from its beginning.
    ///
    /// # Errors
    ///
    /// [`SequenceError::UnknownPlaylist`] when it does not exist.
    pub fn set_active(&mut self, playlist: PlaylistId) -> Result<(), SequenceError> {
        self.require(playlist)?;
        if self.active != Some(playlist) {
            self.active = Some(playlist);
            self.cursor = None;
            self.rebuild_shuffle();
        }
        Ok(())
    }

    /// Adds the entry to the back of the queue. It may be queued more than once.
    ///
    /// # Errors
    ///
    /// [`SequenceError`] when the playlist or entry does not exist.
    pub fn enqueue(&mut self, position: Position) -> Result<(), SequenceError> {
        self.require_entry(position)?;
        self.queue.push_back(position);
        Ok(())
    }

    /// Changes the playback order. Shuffle draws a new permutation, with the entry the order
    /// is at counted as played.
    pub fn set_order(&mut self, order: PlaybackOrder) {
        if self.order != order {
            self.order = order;
            self.rebuild_shuffle();
        }
    }

    /// Sets whether playback stops when the current entry ends.
    pub const fn set_stop_after_current(&mut self, stop: bool) {
        self.stop_after_current = stop;
    }

    fn rebuild_shuffle(&mut self) {
        self.shuffle = None;
        if self.order != PlaybackOrder::Shuffle {
            return;
        }
        let Some(list) = self.active.and_then(|active| find(&self.playlists, active)) else {
            return;
        };
        let entries: Vec<EntryId> = list.entries().iter().map(|entry| entry.id).collect();
        self.shuffle = Some(Shuffle::new(&entries, self.cursor, &mut self.rng));
    }
}
