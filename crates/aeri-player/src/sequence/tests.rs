use std::collections::HashSet;

use aeri_core::Source;
use proptest::prelude::*;

use super::*;

fn track(title: &str) -> Arc<Track> {
    Arc::new(Track {
        source: Source::File(format!("{title}.flac").into()),
        duration: None,
        title: Some(title.to_owned()),
        artist: None,
        album: None,
    })
}

fn tracks(titles: &[&str]) -> Vec<Arc<Track>> {
    titles.iter().map(|title| track(title)).collect()
}

/// A sequence with one playlist holding `titles`, in the given order.
fn with(order: PlaybackOrder, titles: &[&str]) -> (Sequence, PlaylistId, Vec<EntryId>) {
    let mut sequence = Sequence::new(7);
    let list = sequence.create_playlist("list".to_owned());
    let ids = sequence
        .append(list, tracks(titles))
        .expect("the playlist exists");
    sequence.set_order(order);
    (sequence, list, ids)
}

fn at(list: PlaylistId, entry: EntryId) -> Position {
    Position {
        playlist: list,
        entry,
    }
}

fn title(sequence: &Sequence, position: Option<Position>) -> Option<String> {
    let position = position?;
    sequence
        .playlist(position.playlist)?
        .get(position.entry)?
        .track
        .title
        .clone()
}

/// What `advance` yields `count` times, as titles.
fn play(sequence: &mut Sequence, count: usize) -> Vec<Option<String>> {
    (0..count)
        .map(|_| {
            let next = sequence.advance();
            title(sequence, next)
        })
        .collect()
}

fn some(titles: &[&str]) -> Vec<Option<String>> {
    titles
        .iter()
        .map(|title| Some((*title).to_owned()))
        .collect()
}

fn ids_of(sequence: &Sequence, list: PlaylistId) -> Vec<EntryId> {
    sequence
        .playlist(list)
        .expect("the playlist exists")
        .entries()
        .iter()
        .map(|entry| entry.id)
        .collect()
}

fn check(sequence: &Sequence) {
    let mut lists = HashSet::new();
    let mut entries = HashSet::new();
    for list in &sequence.playlists {
        assert!(lists.insert(list.id()), "a playlist id repeats");
        for entry in list.entries() {
            assert!(entries.insert(entry.id), "an entry id repeats");
        }
    }
    let exists = |position: Position| {
        sequence
            .playlist(position.playlist)
            .is_some_and(|list| list.contains(position.entry))
    };
    let active = sequence.active.and_then(|id| sequence.playlist(id));
    assert_eq!(sequence.active.is_some(), active.is_some());
    for queued in &sequence.queue {
        assert!(exists(*queued), "a queued entry is gone");
    }
    if let (Some(cursor), Some(active)) = (sequence.cursor, active) {
        assert!(
            active.contains(cursor),
            "the cursor is not in the active playlist"
        );
    }
    assert!(sequence.cursor.is_none() || active.is_some());
    match (&sequence.shuffle, active) {
        (Some(shuffle), Some(active)) if sequence.order == PlaybackOrder::Shuffle => {
            let mut expected: Vec<_> = active.entries().iter().map(|entry| entry.id).collect();
            expected.sort();
            let (pass, taken, following) = shuffle.parts();
            let sorted = |ids: &[EntryId]| {
                let mut ids = ids.to_vec();
                ids.sort();
                ids
            };
            assert_eq!(sorted(pass), expected, "the pass is not the playlist");
            assert_eq!(
                sorted(following),
                expected,
                "the next pass is not the playlist"
            );
            assert!(taken <= pass.len());
        }
        (None, _) if sequence.order != PlaybackOrder::Shuffle => {}
        (None, None) => {}
        _ => panic!("the shuffle state does not match the order"),
    }
    for next in [
        sequence.peek_next(),
        sequence.peek_skip_next(),
        sequence.peek_previous(),
    ]
    .into_iter()
    .flatten()
    {
        assert!(exists(next), "a query names an entry that is gone");
    }
    if sequence.stop_after_current {
        assert_eq!(sequence.peek_next(), None);
    } else if let Some(front) = sequence.queue.front() {
        assert_eq!(sequence.peek_next(), Some(*front));
    }
}

// ---- playlists and ids ----

#[test]
fn the_first_playlist_becomes_active_and_later_ones_do_not() {
    let mut sequence = Sequence::new(1);
    assert_eq!(sequence.active(), None);

    let first = sequence.create_playlist("one".to_owned());
    let second = sequence.create_playlist("two".to_owned());

    assert_eq!(sequence.active(), Some(first));
    assert_eq!(sequence.playlists().len(), 2);
    assert_ne!(first, second);
}

#[test]
fn append_returns_the_new_ids_in_order_and_keeps_the_same_track_twice() {
    let mut sequence = Sequence::new(1);
    let list = sequence.create_playlist("one".to_owned());
    let song = track("a");

    let ids = sequence
        .append(list, vec![Arc::clone(&song), Arc::clone(&song)])
        .expect("the playlist exists");

    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);
    assert_eq!(ids_of(&sequence, list), ids);
}

#[test]
fn ids_are_never_reused_after_a_removal() {
    let (mut sequence, list, ids) = with(PlaybackOrder::Default, &["a", "b"]);
    sequence.remove(list, &ids).expect("they exist");

    let again = sequence
        .append(list, tracks(&["c"]))
        .expect("the playlist exists");

    assert!(!ids.contains(&again[0]));
}

#[test]
fn a_command_naming_an_unknown_playlist_is_an_error_not_a_panic() {
    let mut other = Sequence::new(1);
    let stranger = other.create_playlist("x".to_owned());
    let mut sequence = Sequence::new(1);

    assert_eq!(
        sequence.append(stranger, vec![]),
        Err(SequenceError::UnknownPlaylist(stranger))
    );
    assert_eq!(
        sequence.set_active(stranger),
        Err(SequenceError::UnknownPlaylist(stranger))
    );
    assert_eq!(
        sequence.clear(stranger),
        Err(SequenceError::UnknownPlaylist(stranger))
    );
    assert!(sequence.playlist(stranger).is_none());
}

#[test]
fn an_error_names_what_is_missing() {
    let mut other = Sequence::new(1);
    let list = other.create_playlist("x".to_owned());
    let entry = other.append(list, tracks(&["a"])).expect("exists")[0];

    assert_eq!(
        SequenceError::UnknownPlaylist(list).to_string(),
        "playlist 1 does not exist"
    );
    assert_eq!(
        SequenceError::UnknownEntry(entry).to_string(),
        "entry 1 is not in the playlist"
    );
}

#[test]
fn removing_an_unknown_entry_changes_nothing() {
    let (mut sequence, list, ids) = with(PlaybackOrder::Default, &["a", "b"]);
    let mut other = Sequence::new(1);
    let other_list = other.create_playlist("x".to_owned());
    let stranger = other
        .append(other_list, tracks(&["a", "b", "c", "d", "e"]))
        .expect("exists")[4];

    assert_eq!(
        sequence.remove(list, &[ids[0], stranger]),
        Err(SequenceError::UnknownEntry(stranger))
    );

    assert_eq!(ids_of(&sequence, list), ids);
}

#[test]
fn removing_the_same_entry_twice_in_one_call_removes_it_once() {
    let (mut sequence, list, ids) = with(PlaybackOrder::Default, &["a", "b"]);

    sequence.remove(list, &[ids[0], ids[0]]).expect("it exists");

    assert_eq!(ids_of(&sequence, list), [ids[1]]);
}

#[test]
fn clear_empties_the_playlist_and_the_queue_of_its_entries() {
    let (mut sequence, list, ids) = with(PlaybackOrder::Default, &["a", "b"]);
    sequence.enqueue(at(list, ids[1])).expect("exists");

    sequence.clear(list).expect("exists");

    assert!(sequence.playlist(list).expect("exists").is_empty());
    assert_eq!(sequence.queue().len(), 0);
    assert_eq!(sequence.peek_next(), None);
}

#[test]
fn moving_an_entry_changes_what_follows_the_cursor() {
    let (mut sequence, list, ids) = with(PlaybackOrder::Default, &["a", "b", "c"]);
    sequence.take(at(list, ids[0]));

    sequence
        .move_before(list, ids[2], Some(ids[1]))
        .expect("both exist");

    assert_eq!(
        play(&mut sequence, 3),
        [Some("c".into()), Some("b".into()), None]
    );
}

#[test]
fn moving_with_an_unknown_id_is_an_error_and_changes_nothing() {
    let (mut sequence, list, ids) = with(PlaybackOrder::Default, &["a", "b"]);
    sequence.remove(list, &[ids[1]]).expect("exists");

    assert_eq!(
        sequence.move_before(list, ids[0], Some(ids[1])),
        Err(SequenceError::UnknownEntry(ids[1]))
    );
    assert_eq!(
        sequence.move_before(list, ids[1], None),
        Err(SequenceError::UnknownEntry(ids[1]))
    );
    assert_eq!(ids_of(&sequence, list), [ids[0]]);
}

#[test]
fn an_edit_does_not_change_a_snapshot_already_handed_out() {
    let (mut sequence, list, _) = with(PlaybackOrder::Default, &["a"]);
    let snapshot = Arc::clone(sequence.playlist(list).expect("exists"));

    sequence.append(list, tracks(&["b"])).expect("exists");

    assert_eq!(snapshot.len(), 1);
    assert_eq!(sequence.playlist(list).expect("exists").len(), 2);
}

// ---- default order ----

#[test]
fn default_order_visits_each_entry_once_then_ends() {
    let (mut sequence, _, _) = with(PlaybackOrder::Default, &["a", "b", "c"]);

    assert_eq!(
        play(&mut sequence, 5),
        [
            Some("a".into()),
            Some("b".into()),
            Some("c".into()),
            None,
            None
        ]
    );
}

#[test]
fn nothing_plays_with_no_playlist_or_an_empty_one() {
    let mut sequence = Sequence::new(1);
    assert_eq!(sequence.advance(), None);
    assert_eq!(sequence.peek_previous(), None);

    sequence.create_playlist("empty".to_owned());

    assert_eq!(sequence.advance(), None);
    assert_eq!(sequence.peek_skip_next(), None);
}

#[test]
fn peeking_does_not_change_the_answer() {
    let (mut sequence, list, ids) = with(PlaybackOrder::Default, &["a", "b", "c"]);
    sequence.take(at(list, ids[0]));

    assert_eq!(sequence.peek_next(), sequence.peek_next());
    assert_eq!(sequence.peek_next(), Some(at(list, ids[1])));
    assert_eq!(sequence.peek_skip_next(), Some(at(list, ids[1])));
    assert_eq!(sequence.current(), Some(at(list, ids[0])));
}

// ---- repeat ----

#[test]
fn repeat_track_plays_the_same_entry_until_the_user_skips() {
    let (mut sequence, list, ids) = with(PlaybackOrder::RepeatTrack, &["a", "b", "c"]);

    assert_eq!(play(&mut sequence, 3), some(&["a", "a", "a"]));
    assert_eq!(sequence.peek_skip_next(), Some(at(list, ids[1])));
    sequence.take(at(list, ids[1]));
    assert_eq!(play(&mut sequence, 2), some(&["b", "b"]));
}

#[test]
fn skipping_past_the_last_entry_under_repeat_track_finds_nothing() {
    let (mut sequence, list, ids) = with(PlaybackOrder::RepeatTrack, &["a", "b"]);
    sequence.take(at(list, ids[1]));

    assert_eq!(sequence.peek_skip_next(), None);
    assert_eq!(sequence.peek_next(), Some(at(list, ids[1])));
}

#[test]
fn repeat_playlist_wraps_forwards_and_never_ends() {
    let (mut sequence, list, ids) = with(PlaybackOrder::RepeatPlaylist, &["a", "b", "c"]);

    assert_eq!(
        play(&mut sequence, 7),
        some(&["a", "b", "c", "a", "b", "c", "a"])
    );
    sequence.take(at(list, ids[2]));
    assert_eq!(sequence.peek_skip_next(), Some(at(list, ids[0])));
}

#[test]
fn repeat_playlist_with_one_entry_repeats_it() {
    let (mut sequence, _, _) = with(PlaybackOrder::RepeatPlaylist, &["only"]);

    assert_eq!(play(&mut sequence, 3), some(&["only", "only", "only"]));
}

// ---- the queue ----

#[test]
fn a_queued_entry_plays_before_the_order_and_the_order_then_resumes() {
    let (mut sequence, list, ids) = with(PlaybackOrder::Default, &["a", "b", "c", "d"]);
    sequence.take(at(list, ids[0]));
    sequence.enqueue(at(list, ids[3])).expect("exists");
    sequence.enqueue(at(list, ids[2])).expect("exists");

    assert_eq!(play(&mut sequence, 4), some(&["d", "c", "b", "c"]));
}

#[test]
fn after_queued_tracks_repeat_track_repeats_the_entry_from_before_the_queue() {
    let (mut sequence, list, ids) = with(PlaybackOrder::RepeatTrack, &["a", "b", "c"]);
    sequence.take(at(list, ids[1]));
    sequence.enqueue(at(list, ids[2])).expect("exists");

    assert_eq!(play(&mut sequence, 3), some(&["c", "b", "b"]));
}

#[test]
fn an_entry_can_be_queued_twice() {
    let (mut sequence, list, ids) = with(PlaybackOrder::Default, &["a", "b"]);
    sequence.enqueue(at(list, ids[1])).expect("exists");
    sequence.enqueue(at(list, ids[1])).expect("exists");

    assert_eq!(play(&mut sequence, 3), some(&["b", "b", "a"]));
}

#[test]
fn queueing_works_across_playlists_and_the_order_stays_in_the_active_one() {
    let (mut sequence, list, ids) = with(PlaybackOrder::Default, &["a", "b"]);
    let other = sequence.create_playlist("other".to_owned());
    let foreign = sequence.append(other, tracks(&["x"])).expect("exists")[0];
    sequence.take(at(list, ids[0]));
    sequence.enqueue(at(other, foreign)).expect("exists");

    assert_eq!(sequence.advance(), Some(at(other, foreign)));
    assert_eq!(sequence.active(), Some(list));
    assert_eq!(play(&mut sequence, 1), some(&["b"]));
}

#[test]
fn removing_a_queued_entry_from_its_playlist_removes_it_from_the_queue() {
    let (mut sequence, list, ids) = with(PlaybackOrder::Default, &["a", "b", "c"]);
    sequence.enqueue(at(list, ids[1])).expect("exists");
    sequence.enqueue(at(list, ids[2])).expect("exists");

    sequence.remove(list, &[ids[1]]).expect("exists");

    assert_eq!(sequence.queue().collect::<Vec<_>>(), [at(list, ids[2])]);
}

#[test]
fn queueing_an_unknown_entry_is_an_error() {
    let (mut sequence, list, ids) = with(PlaybackOrder::Default, &["a"]);
    sequence.remove(list, &ids).expect("exists");

    assert_eq!(
        sequence.enqueue(at(list, ids[0])),
        Err(SequenceError::UnknownEntry(ids[0]))
    );
    assert_eq!(sequence.queue().len(), 0);
}

#[test]
fn taking_an_entry_that_is_gone_is_ignored() {
    let (mut sequence, list, ids) = with(PlaybackOrder::Default, &["a", "b"]);
    sequence.take(at(list, ids[0]));
    sequence.remove(list, &[ids[1]]).expect("exists");

    sequence.take(at(list, ids[1]));

    assert_eq!(sequence.current(), Some(at(list, ids[0])));
    assert_eq!(sequence.peek_next(), None);
}

// ---- stop after current ----

#[test]
fn stop_after_current_ends_the_natural_advance_and_then_clears() {
    let (mut sequence, list, ids) = with(PlaybackOrder::Default, &["a", "b"]);
    sequence.take(at(list, ids[0]));
    sequence.set_stop_after_current(true);

    assert!(sequence.stop_after_current());
    assert_eq!(sequence.peek_next(), None);
    assert_eq!(sequence.advance(), None);
    assert!(!sequence.stop_after_current());
    assert_eq!(sequence.peek_next(), Some(at(list, ids[1])));
}

#[test]
fn stop_after_current_beats_the_queue_but_not_a_skip() {
    let (mut sequence, list, ids) = with(PlaybackOrder::Default, &["a", "b"]);
    sequence.take(at(list, ids[0]));
    sequence.enqueue(at(list, ids[1])).expect("exists");
    sequence.set_stop_after_current(true);

    assert_eq!(sequence.peek_next(), None);
    assert_eq!(sequence.peek_skip_next(), Some(at(list, ids[1])));
}

#[test]
fn stopped_clears_the_flag_and_nothing_else() {
    let (mut sequence, list, ids) = with(PlaybackOrder::Default, &["a", "b"]);
    sequence.take(at(list, ids[0]));
    sequence.set_stop_after_current(true);

    sequence.stopped();

    assert!(!sequence.stop_after_current());
    assert_eq!(sequence.current(), Some(at(list, ids[0])));
}

// ---- previous ----

#[test]
fn previous_goes_to_the_entry_before_and_never_restarts_the_current_one() {
    let (mut sequence, list, ids) = with(PlaybackOrder::Default, &["a", "b", "c"]);
    sequence.take(at(list, ids[2]));

    assert_eq!(sequence.peek_previous(), Some(at(list, ids[1])));
    sequence.take(at(list, ids[1]));
    assert_eq!(sequence.peek_previous(), Some(at(list, ids[0])));
    sequence.take(at(list, ids[0]));
    assert_eq!(sequence.peek_previous(), None);
}

#[test]
fn previous_under_repeat_track_still_goes_back_a_track() {
    let (mut sequence, list, ids) = with(PlaybackOrder::RepeatTrack, &["a", "b"]);
    sequence.take(at(list, ids[1]));

    assert_eq!(sequence.peek_previous(), Some(at(list, ids[0])));
    sequence.take(at(list, ids[0]));
    assert_eq!(sequence.peek_previous(), None);
}

#[test]
fn previous_under_repeat_playlist_wraps_to_the_last_entry() {
    let (mut sequence, list, ids) = with(PlaybackOrder::RepeatPlaylist, &["a", "b", "c"]);
    sequence.take(at(list, ids[0]));

    assert_eq!(sequence.peek_previous(), Some(at(list, ids[2])));
}

#[test]
fn previous_from_a_queued_entry_goes_to_the_entry_it_interrupted() {
    let (mut sequence, list, ids) = with(PlaybackOrder::Default, &["a", "b", "c"]);
    sequence.take(at(list, ids[1]));
    sequence.enqueue(at(list, ids[2])).expect("exists");
    sequence.advance();

    assert_eq!(sequence.peek_previous(), Some(at(list, ids[1])));
}

#[test]
fn previous_with_nothing_taken_finds_nothing() {
    let (sequence, _, _) = with(PlaybackOrder::Default, &["a", "b"]);

    assert_eq!(sequence.peek_previous(), None);
}

// ---- removal and the cursor ----

#[test]
fn removing_the_entry_the_order_is_at_continues_with_its_successor() {
    let (mut sequence, list, ids) = with(PlaybackOrder::Default, &["a", "b", "c", "d"]);
    sequence.take(at(list, ids[1]));

    sequence.remove(list, &[ids[1]]).expect("exists");

    assert_eq!(sequence.peek_next(), Some(at(list, ids[2])));
    assert_eq!(sequence.peek_previous(), Some(at(list, ids[0])));
}

#[test]
fn removing_the_first_entry_while_it_is_the_cursor_continues_with_the_new_first() {
    let (mut sequence, list, ids) = with(PlaybackOrder::Default, &["a", "b"]);
    sequence.take(at(list, ids[0]));

    sequence.remove(list, &[ids[0]]).expect("exists");

    assert_eq!(sequence.peek_next(), Some(at(list, ids[1])));
}

#[test]
fn removing_both_the_cursor_and_the_one_before_it_still_continues_with_the_successor() {
    let (mut sequence, list, ids) = with(PlaybackOrder::Default, &["a", "b", "c", "d"]);
    sequence.take(at(list, ids[2]));

    sequence.remove(list, &[ids[2], ids[1]]).expect("exist");

    assert_eq!(sequence.peek_next(), Some(at(list, ids[3])));
}

#[test]
fn removing_other_entries_leaves_the_cursor_alone() {
    let (mut sequence, list, ids) = with(PlaybackOrder::Default, &["a", "b", "c"]);
    sequence.take(at(list, ids[1]));

    sequence.remove(list, &[ids[0]]).expect("exists");

    assert_eq!(sequence.peek_next(), Some(at(list, ids[2])));
}

#[test]
fn removing_from_a_playlist_that_is_not_active_leaves_the_cursor_alone() {
    let (mut sequence, list, ids) = with(PlaybackOrder::Default, &["a", "b"]);
    let other = sequence.create_playlist("other".to_owned());
    let foreign = sequence.append(other, tracks(&["x"])).expect("exists");
    sequence.take(at(list, ids[0]));

    sequence.remove(other, &foreign).expect("exists");

    assert_eq!(sequence.peek_next(), Some(at(list, ids[1])));
}

// ---- active playlist and jumping ----

#[test]
fn switching_the_active_playlist_starts_its_order_from_the_beginning() {
    let (mut sequence, list, ids) = with(PlaybackOrder::Default, &["a", "b"]);
    let other = sequence.create_playlist("other".to_owned());
    let foreign = sequence.append(other, tracks(&["x", "y"])).expect("exists");
    sequence.take(at(list, ids[1]));

    sequence.set_active(other).expect("exists");

    assert_eq!(sequence.active(), Some(other));
    assert_eq!(sequence.peek_next(), Some(at(other, foreign[0])));
    sequence.set_active(other).expect("exists again");
    assert_eq!(sequence.peek_next(), Some(at(other, foreign[0])));
}

#[test]
fn taking_an_entry_from_another_playlist_makes_that_playlist_active() {
    let (mut sequence, list, _) = with(PlaybackOrder::Default, &["a"]);
    let other = sequence.create_playlist("other".to_owned());
    let foreign = sequence.append(other, tracks(&["x", "y"])).expect("exists");
    assert_eq!(sequence.active(), Some(list));

    sequence.take(at(other, foreign[0]));

    assert_eq!(sequence.active(), Some(other));
    assert_eq!(sequence.peek_next(), Some(at(other, foreign[1])));
}

#[test]
fn jumping_plays_that_entry_and_the_order_continues_after_it() {
    let (mut sequence, list, ids) = with(PlaybackOrder::Default, &["a", "b", "c"]);

    sequence.jump_to(at(list, ids[1])).expect("exists");

    assert_eq!(sequence.current(), Some(at(list, ids[1])));
    assert_eq!(sequence.peek_next(), Some(at(list, ids[2])));
}

#[test]
fn jumping_to_the_front_of_the_queue_does_not_consume_it() {
    let (mut sequence, list, ids) = with(PlaybackOrder::Default, &["a", "b"]);
    sequence.enqueue(at(list, ids[0])).expect("exists");

    sequence.jump_to(at(list, ids[0])).expect("exists");

    assert_eq!(sequence.queue().len(), 1);
}

#[test]
fn jumping_to_something_that_is_not_there_is_an_error() {
    let (mut sequence, list, ids) = with(PlaybackOrder::Default, &["a"]);
    sequence.remove(list, &ids).expect("exists");

    assert_eq!(
        sequence.jump_to(at(list, ids[0])),
        Err(SequenceError::UnknownEntry(ids[0]))
    );
    assert_eq!(sequence.current(), None);
}

// ---- shuffle ----

fn shuffled(seed: u64, count: usize) -> (Sequence, PlaylistId, Vec<EntryId>) {
    let titles: Vec<String> = (0..count).map(|n| format!("t{n}")).collect();
    let titles: Vec<&str> = titles.iter().map(String::as_str).collect();
    let mut sequence = Sequence::new(seed);
    let list = sequence.create_playlist("list".to_owned());
    let ids = sequence.append(list, tracks(&titles)).expect("exists");
    sequence.set_order(PlaybackOrder::Shuffle);
    (sequence, list, ids)
}

fn take_entries(sequence: &mut Sequence, count: usize) -> Vec<EntryId> {
    (0..count)
        .map(|_| sequence.advance().expect("shuffle never ends").entry)
        .collect()
}

#[test]
fn a_shuffle_pass_visits_every_entry_exactly_once_and_then_another_pass_begins() {
    let (mut sequence, _, ids) = shuffled(3, 9);

    let first = take_entries(&mut sequence, 9);
    let second = take_entries(&mut sequence, 9);

    for pass in [&first, &second] {
        let mut sorted = pass.clone();
        sorted.sort();
        let mut expected = ids.clone();
        expected.sort();
        assert_eq!(sorted, expected);
    }
}

#[test]
fn the_same_seed_gives_the_same_shuffle_and_another_seed_a_different_one() {
    let order = |seed| {
        let (mut sequence, _, _) = shuffled(seed, 12);
        take_entries(&mut sequence, 12)
    };

    assert_eq!(order(5), order(5));
    assert_ne!(order(5), order(6));
}

#[test]
fn a_new_pass_never_starts_with_the_entry_the_last_one_ended_on() {
    for seed in 0..40 {
        let (mut sequence, _, _) = shuffled(seed, 3);
        let played = take_entries(&mut sequence, 30);

        for pair in played.windows(2) {
            assert_ne!(pair[0], pair[1], "seed {seed}");
        }
    }
}

#[test]
fn a_shuffle_of_one_entry_repeats_it() {
    let (mut sequence, _, ids) = shuffled(1, 1);

    assert_eq!(take_entries(&mut sequence, 3), [ids[0], ids[0], ids[0]]);
}

#[test]
fn shuffling_counts_the_entry_playing_as_already_played() {
    let (mut sequence, list, ids) = with(PlaybackOrder::Default, &["a", "b", "c", "d", "e"]);
    sequence.take(at(list, ids[2]));

    sequence.set_order(PlaybackOrder::Shuffle);

    let rest = take_entries(&mut sequence, 4);
    assert!(!rest.contains(&ids[2]));
    assert_eq!(rest.iter().collect::<HashSet<_>>().len(), 4);
    check(&sequence);
}

#[test]
fn an_entry_added_mid_pass_plays_in_this_pass_and_in_the_next() {
    for seed in 0..20 {
        let (mut sequence, list, _) = shuffled(seed, 6);
        take_entries(&mut sequence, 2);
        let added = sequence.append(list, tracks(&["new"])).expect("exists")[0];

        let rest = take_entries(&mut sequence, 5);
        let next_pass = take_entries(&mut sequence, 7);

        assert!(rest.contains(&added), "seed {seed}");
        assert_eq!(rest.iter().collect::<HashSet<_>>().len(), 5);
        assert!(next_pass.contains(&added), "seed {seed}");
        check(&sequence);
    }
}

#[test]
fn an_entry_removed_mid_pass_is_not_played_in_this_pass_or_the_next() {
    for seed in 0..20 {
        let (mut sequence, list, ids) = shuffled(seed, 6);
        let played = take_entries(&mut sequence, 2);
        let victim = *ids
            .iter()
            .find(|id| !played.contains(id))
            .expect("some left");

        sequence.remove(list, &[victim]).expect("exists");

        let rest = take_entries(&mut sequence, 4);
        let next_pass = take_entries(&mut sequence, 5);
        assert!(!rest.contains(&victim) && !next_pass.contains(&victim));
        check(&sequence);
    }
}

#[test]
fn removing_a_played_entry_mid_pass_keeps_the_pass_in_step() {
    let (mut sequence, list, _) = shuffled(2, 6);
    let played = take_entries(&mut sequence, 3);

    sequence.remove(list, &[played[0]]).expect("exists");

    let rest = take_entries(&mut sequence, 3);
    let mut all = played[1..].to_vec();
    all.extend(&rest);
    assert_eq!(all.iter().collect::<HashSet<_>>().len(), 5);
    check(&sequence);
}

#[test]
fn removing_the_next_unplayed_entry_under_shuffle_does_not_skip_the_one_after_it() {
    for seed in 0..20 {
        let (mut sequence, list, _) = shuffled(seed, 6);
        take_entries(&mut sequence, 2);
        let (pass, taken, _) = sequence
            .shuffle
            .as_ref()
            .map(Shuffle::parts)
            .expect("shuffle");
        let (next, after) = (pass[taken], pass[taken + 1]);

        sequence.remove(list, &[next]).expect("exists");

        assert_eq!(sequence.peek_next(), Some(at(list, after)), "seed {seed}");
    }
}

#[test]
fn removing_the_entry_playing_under_shuffle_moves_the_cursor_back_within_the_pass() {
    let (mut sequence, list, _) = shuffled(4, 6);
    let played = take_entries(&mut sequence, 3);

    sequence.remove(list, &[played[2]]).expect("exists");

    assert_eq!(sequence.cursor, Some(played[1]));
    check(&sequence);
}

#[test]
fn previous_in_shuffle_goes_back_within_the_pass_and_stops_at_its_start() {
    let (mut sequence, list, _) = shuffled(9, 6);
    let played = take_entries(&mut sequence, 3);

    assert_eq!(sequence.peek_previous(), Some(at(list, played[1])));
    sequence.take(at(list, played[1]));
    assert_eq!(sequence.peek_previous(), Some(at(list, played[0])));
    sequence.take(at(list, played[0]));
    assert_eq!(sequence.peek_previous(), None);
    check(&sequence);
}

#[test]
fn going_back_in_shuffle_and_then_forward_replays_the_pass_in_order() {
    let (mut sequence, list, _) = shuffled(11, 6);
    let played = take_entries(&mut sequence, 3);
    sequence.take(at(list, played[1]));

    assert_eq!(sequence.peek_next(), Some(at(list, played[2])));
}

#[test]
fn jumping_to_any_entry_under_shuffle_makes_it_the_last_taken() {
    let (mut sequence, list, ids) = shuffled(5, 6);
    take_entries(&mut sequence, 4);

    sequence.jump_to(at(list, ids[3])).expect("exists");

    assert_eq!(sequence.cursor, Some(ids[3]));
    assert_eq!(
        sequence.shuffle.as_ref().and_then(Shuffle::last_taken),
        Some(ids[3])
    );
    check(&sequence);
}

#[test]
fn jumping_to_an_unplayed_entry_under_shuffle_keeps_the_others_in_order() {
    let (mut sequence, list, _) = shuffled(8, 6);
    take_entries(&mut sequence, 1);
    let upcoming = sequence
        .shuffle
        .as_ref()
        .map(|s| s.parts().0[1..].to_vec())
        .expect("shuffle");

    sequence.jump_to(at(list, upcoming[2])).expect("exists");

    let rest = take_entries(&mut sequence, 4);
    let expected: Vec<_> = upcoming
        .iter()
        .copied()
        .filter(|id| *id != upcoming[2])
        .collect();
    assert_eq!(rest, expected);
}

#[test]
fn leaving_shuffle_drops_its_state_and_the_default_order_resumes_from_the_cursor() {
    let (mut sequence, list, ids) = shuffled(2, 4);
    sequence.jump_to(at(list, ids[1])).expect("exists");

    sequence.set_order(PlaybackOrder::Default);

    assert!(sequence.shuffle.is_none());
    assert_eq!(sequence.peek_next(), Some(at(list, ids[2])));
    check(&sequence);
}

#[test]
fn switching_the_active_playlist_under_shuffle_shuffles_the_new_one() {
    let (mut sequence, _, _) = shuffled(2, 4);
    let other = sequence.create_playlist("other".to_owned());
    let foreign = sequence
        .append(other, tracks(&["x", "y", "z"]))
        .expect("exists");

    sequence.set_active(other).expect("exists");

    let pass = take_entries(&mut sequence, 3);
    assert_eq!(
        pass.iter().collect::<HashSet<_>>(),
        foreign.iter().collect::<HashSet<_>>()
    );
    check(&sequence);
}

#[test]
fn creating_the_first_playlist_under_shuffle_starts_an_empty_shuffle() {
    let mut sequence = Sequence::new(1);
    sequence.set_order(PlaybackOrder::Shuffle);
    let list = sequence.create_playlist("list".to_owned());

    assert_eq!(sequence.peek_next(), None);
    let ids = sequence.append(list, tracks(&["a", "b"])).expect("exists");
    assert_eq!(take_entries(&mut sequence, 2).len(), 2);
    assert_eq!(ids.len(), 2);
    check(&sequence);
}

#[test]
fn setting_the_same_order_again_changes_nothing() {
    let (mut sequence, _, _) = shuffled(3, 5);
    let before = take_entries(&mut sequence, 2);
    let upcoming = sequence.peek_next();

    sequence.set_order(PlaybackOrder::Shuffle);

    assert_eq!(sequence.peek_next(), upcoming);
    assert_eq!(before.len(), 2);
}

#[test]
fn the_default_order_is_default() {
    assert_eq!(PlaybackOrder::default(), PlaybackOrder::Default);
}

// ---- properties ----

#[derive(Debug, Clone)]
enum Op {
    Create,
    Append {
        list: usize,
        count: usize,
    },
    Remove {
        list: usize,
        picks: Vec<usize>,
    },
    Clear {
        list: usize,
    },
    Move {
        list: usize,
        entry: usize,
        before: Option<usize>,
    },
    SetActive(usize),
    Enqueue {
        list: usize,
        entry: usize,
    },
    SetOrder(usize),
    Stop(bool),
    Advance,
    Jump {
        list: usize,
        entry: usize,
    },
    Previous,
    SkipNext,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        2 => Just(Op::Create),
        6 => (any::<usize>(), 1_usize..4).prop_map(|(list, count)| Op::Append { list, count }),
        3 => (any::<usize>(), prop::collection::vec(any::<usize>(), 1..3))
            .prop_map(|(list, picks)| Op::Remove { list, picks }),
        1 => any::<usize>().prop_map(|list| Op::Clear { list }),
        2 => (any::<usize>(), any::<usize>(), prop::option::of(any::<usize>()))
            .prop_map(|(list, entry, before)| Op::Move { list, entry, before }),
        2 => any::<usize>().prop_map(Op::SetActive),
        3 => (any::<usize>(), any::<usize>()).prop_map(|(list, entry)| Op::Enqueue { list, entry }),
        3 => (0_usize..4).prop_map(Op::SetOrder),
        1 => any::<bool>().prop_map(Op::Stop),
        8 => Just(Op::Advance),
        2 => (any::<usize>(), any::<usize>()).prop_map(|(list, entry)| Op::Jump { list, entry }),
        2 => Just(Op::Previous),
        2 => Just(Op::SkipNext),
    ]
}

fn pick_list(sequence: &Sequence, index: usize) -> Option<PlaylistId> {
    let lists = sequence.playlists();
    (!lists.is_empty()).then(|| lists[index % lists.len()].id())
}

fn pick_entry(sequence: &Sequence, list: PlaylistId, index: usize) -> Option<EntryId> {
    let entries = sequence.playlist(list)?.entries();
    (!entries.is_empty()).then(|| entries[index % entries.len()].id)
}

fn apply(sequence: &mut Sequence, op: &Op) {
    match op {
        Op::Create => {
            sequence.create_playlist("p".to_owned());
        }
        Op::Append { list, count } => {
            if let Some(list) = pick_list(sequence, *list) {
                let titles = vec!["t"; *count];
                sequence.append(list, tracks(&titles)).expect("exists");
            }
        }
        Op::Remove { list, picks } => {
            if let Some(list) = pick_list(sequence, *list) {
                let ids: Vec<_> = picks
                    .iter()
                    .filter_map(|pick| pick_entry(sequence, list, *pick))
                    .collect();
                sequence.remove(list, &ids).expect("they exist");
            }
        }
        Op::Clear { list } => {
            if let Some(list) = pick_list(sequence, *list) {
                sequence.clear(list).expect("exists");
            }
        }
        Op::Move {
            list,
            entry,
            before,
        } => {
            if let Some(list) = pick_list(sequence, *list)
                && let Some(entry) = pick_entry(sequence, list, *entry)
            {
                let before = before.and_then(|before| pick_entry(sequence, list, before));
                sequence.move_before(list, entry, before).expect("exist");
            }
        }
        Op::SetActive(list) => {
            if let Some(list) = pick_list(sequence, *list) {
                sequence.set_active(list).expect("exists");
            }
        }
        Op::Enqueue { list, entry } => {
            if let Some(list) = pick_list(sequence, *list)
                && let Some(entry) = pick_entry(sequence, list, *entry)
            {
                sequence.enqueue(at(list, entry)).expect("exists");
            }
        }
        Op::SetOrder(index) => sequence.set_order(
            [
                PlaybackOrder::Default,
                PlaybackOrder::RepeatTrack,
                PlaybackOrder::RepeatPlaylist,
                PlaybackOrder::Shuffle,
            ][*index % 4],
        ),
        Op::Stop(stop) => sequence.set_stop_after_current(*stop),
        Op::Advance => {
            let front = sequence.queue().next();
            let stopping = sequence.stop_after_current();
            let taken = sequence.advance();
            if !stopping && front.is_some() {
                assert_eq!(taken, front, "a queued entry plays before any other");
            }
            if stopping {
                assert_eq!(taken, None);
                assert!(!sequence.stop_after_current());
            }
        }
        Op::Jump { list, entry } => {
            if let Some(list) = pick_list(sequence, *list)
                && let Some(entry) = pick_entry(sequence, list, *entry)
            {
                sequence.jump_to(at(list, entry)).expect("exists");
                assert_eq!(sequence.current(), Some(at(list, entry)));
            }
        }
        Op::Previous => {
            if let Some(previous) = sequence.peek_previous() {
                sequence.take(previous);
            }
        }
        Op::SkipNext => {
            if let Some(next) = sequence.peek_skip_next() {
                sequence.take(next);
            }
        }
    }
}

proptest! {
    #[test]
    fn the_invariants_hold_after_every_edit_and_advance(
        seed in any::<u64>(),
        ops in prop::collection::vec(op(), 0..80),
    ) {
        let mut sequence = Sequence::new(seed);
        for op in &ops {
            apply(&mut sequence, op);
            check(&sequence);
        }
    }

    #[test]
    fn default_order_visits_each_entry_once_in_order_and_ends(count in 1_usize..40) {
        let titles = vec!["t"; count];
        let (mut sequence, list, ids) = with(PlaybackOrder::Default, &titles);

        let visited: Vec<_> = (0..count).map(|_| sequence.advance()).collect();

        let expected: Vec<_> = ids.iter().map(|id| Some(at(list, *id))).collect();
        prop_assert_eq!(visited, expected);
        prop_assert_eq!(sequence.advance(), None);
        prop_assert_eq!(sequence.advance(), None);
    }

    #[test]
    fn repeat_playlist_never_ends_on_a_non_empty_playlist(count in 1_usize..20, steps in 1_usize..100) {
        let titles = vec!["t"; count];
        let (mut sequence, _, ids) = with(PlaybackOrder::RepeatPlaylist, &titles);

        for step in 0..steps {
            let next = sequence.advance();
            prop_assert_eq!(next.map(|p| p.entry), Some(ids[step % count]));
        }
    }

    #[test]
    fn every_shuffle_pass_is_a_permutation_with_no_repeat_across_the_boundary(
        seed in any::<u64>(),
        count in 1_usize..25,
    ) {
        let (mut sequence, _, ids) = shuffled(seed, count);
        let mut sorted_ids = ids;
        sorted_ids.sort();

        let mut previous_last = None;
        for _ in 0..3 {
            let pass = take_entries(&mut sequence, count);
            let mut sorted = pass.clone();
            sorted.sort();
            prop_assert_eq!(&sorted, &sorted_ids);
            if count > 1 {
                prop_assert_ne!(previous_last, pass.first().copied());
            }
            previous_last = pass.last().copied();
        }
    }

    #[test]
    fn a_shuffle_pass_visits_each_entry_once_whatever_is_added_or_removed_mid_pass(
        seed in any::<u64>(),
        count in 2_usize..12,
        edits in prop::collection::vec((any::<bool>(), any::<usize>()), 0..30),
    ) {
        let (mut sequence, list, ids) = shuffled(seed, count);
        let mut unplayed: HashSet<EntryId> = ids.iter().copied().collect();

        for (add, pick) in edits {
            if unplayed.is_empty() {
                unplayed = ids_of(&sequence, list).into_iter().collect();
            }
            let next = sequence
                .advance()
                .expect("shuffle never ends on a non-empty playlist")
                .entry;
            prop_assert!(unplayed.remove(&next), "played an entry that was not due in this pass");

            if add {
                let added = sequence.append(list, tracks(&["new"])).expect("exists")[0];
                unplayed.insert(added);
            } else if let Some(victim) = pick_entry(&sequence, list, pick)
                && sequence.playlist(list).expect("exists").len() > 1
            {
                unplayed.remove(&victim);
                sequence.remove(list, &[victim]).expect("exists");
            }
            check(&sequence);
        }
    }
}
