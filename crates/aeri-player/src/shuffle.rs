//! The permutation a shuffled playlist is played in.

use fastrand::Rng;

use crate::ids::EntryId;

/// Two passes over the playlist: the one being played, and the one after it, so the answer to
/// "what next" at the end of a pass is known without drawing (ADR 0024).
///
/// Both are permutations of the playlist's entries. `taken` is how many of `pass` have been
/// played, so `pass[taken - 1]` is the last of them.
#[derive(Debug)]
pub(crate) struct Shuffle {
    pass: Vec<EntryId>,
    taken: usize,
    following: Vec<EntryId>,
}

fn draw(entries: &[EntryId], avoid_first: Option<EntryId>, rng: &mut Rng) -> Vec<EntryId> {
    let mut drawn = entries.to_vec();
    rng.shuffle(&mut drawn);
    if drawn.len() > 1 && drawn.first().copied() == avoid_first {
        let other = rng.usize(1..drawn.len());
        drawn.swap(0, other);
    }
    drawn
}

impl Shuffle {
    /// A new shuffle of `entries`. When `played` is one of them it counts as already played.
    pub(crate) fn new(entries: &[EntryId], played: Option<EntryId>, rng: &mut Rng) -> Self {
        let mut shuffle = Self {
            pass: draw(entries, None, rng),
            taken: 0,
            following: Vec::new(),
        };
        if let Some(played) = played {
            shuffle.take(played, rng);
        }
        shuffle.following = draw(entries, shuffle.pass.last().copied(), rng);
        shuffle
    }

    /// The entry after the last one taken. The first of the following pass at a pass's end.
    pub(crate) fn peek(&self) -> Option<EntryId> {
        self.pass
            .get(self.taken)
            .or_else(|| self.following.first())
            .copied()
    }

    /// The entry taken before the last one, within this pass.
    pub(crate) fn peek_previous(&self) -> Option<EntryId> {
        self.taken
            .checked_sub(2)
            .and_then(|at| self.pass.get(at))
            .copied()
    }

    pub(crate) fn last_taken(&self) -> Option<EntryId> {
        self.taken
            .checked_sub(1)
            .and_then(|at| self.pass.get(at))
            .copied()
    }

    /// Records that `entry` is now the one playing: the next one, the one before the last
    /// taken, or any other entry, which then counts as the last taken.
    pub(crate) fn take(&mut self, entry: EntryId, rng: &mut Rng) {
        if self.pass.get(self.taken) == Some(&entry) {
            self.taken += 1;
        } else if self.taken >= self.pass.len() && self.following.first() == Some(&entry) {
            self.pass = std::mem::take(&mut self.following);
            self.taken = 1;
            self.following = draw(&self.pass, self.pass.last().copied(), rng);
        } else if self.taken >= 2 && self.pass.get(self.taken - 2) == Some(&entry) {
            self.taken -= 1;
        } else if let Some(at) = self.pass.iter().position(|&other| other == entry) {
            self.pull(at);
            self.pass.insert(self.taken, entry);
            self.taken += 1;
        }
    }

    /// Takes the entry at `at` out of this pass, keeping `taken` pointing at the same place.
    fn pull(&mut self, at: usize) {
        self.pass.remove(at);
        if at < self.taken {
            self.taken -= 1;
        }
    }

    /// A new entry joins the unplayed part of this pass, and the next pass, at random places.
    pub(crate) fn add(&mut self, entry: EntryId, rng: &mut Rng) {
        self.pass
            .insert(rng.usize(self.taken..=self.pass.len()), entry);
        self.following
            .insert(rng.usize(0..=self.following.len()), entry);
    }

    pub(crate) fn remove(&mut self, entry: EntryId) {
        if let Some(at) = self.pass.iter().position(|&other| other == entry) {
            self.pull(at);
        }
        self.following.retain(|&other| other != entry);
    }

    #[cfg(test)]
    pub(crate) fn parts(&self) -> (&[EntryId], usize, &[EntryId]) {
        (&self.pass, self.taken, &self.following)
    }
}
