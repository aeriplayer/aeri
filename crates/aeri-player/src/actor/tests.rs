//! The actor driven directly, with events made by hand, for what the engine cannot be made to
//! produce on demand: stale and failing events, and the bookkeeping behind the slot.

use std::num::NonZeroUsize;
use std::path::PathBuf;

use aeri_core::Source;
use aeri_engine::{FakeSink, FakeSinkObserver};

use super::*;
use crate::ids::{EntryId, PlaylistId};

fn track(file: &str, title: &str) -> Arc<Track> {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "aeri-decode",
        "tests",
        "fixtures",
        file,
    ]
    .iter()
    .collect();
    Arc::new(Track {
        source: Source::File(path),
        duration: None,
        title: Some(title.to_owned()),
        artist: None,
        album: None,
    })
}

struct Harness {
    actor: Actor,
    observer: FakeSinkObserver,
    list: PlaylistId,
    ids: Vec<EntryId>,
    _inbox: mpsc::UnboundedReceiver<Message>,
}

impl Harness {
    fn at(&self, index: usize) -> Position {
        Position {
            playlist: self.list,
            entry: self.ids[index],
        }
    }

    fn ask<T>(&mut self, command: impl FnOnce(Reply<T>) -> Command) -> Result<T, PlayerError> {
        let (reply, mut response) = tokio::sync::oneshot::channel();
        self.actor.handle(command(reply));
        response.try_recv().expect("the command was answered")
    }

    fn play(&mut self) {
        self.ask(Command::Play).expect("the fixture plays");
    }

    fn calls(&self) -> usize {
        self.observer.calls().len()
    }
}

fn harness(files: &[&str]) -> Harness {
    let sink = FakeSink::new(NonZeroUsize::new(4_096).expect("non-zero"));
    let observer = sink.observer();
    observer.hold();
    let (inbox, receiver) = mpsc::unbounded_channel();
    let engine = Engine::spawn(Box::new(sink), inbox).expect("the engine starts");
    let mut sequence = Sequence::new(1);
    let list = sequence.create_playlist("list".to_owned());
    let tracks = files
        .iter()
        .enumerate()
        .map(|(index, file)| track(file, &format!("t{index}")))
        .collect();
    let ids = sequence.append(list, tracks).expect("exists");
    let (publisher, _) = watch::channel(Actor::snapshot(
        &sequence,
        None,
        Volume::UNITY,
        ReplayGainMode::default(),
    ));
    Harness {
        actor: Actor::new(
            sequence,
            engine,
            publisher,
            Volume::UNITY,
            ReplayGainMode::default(),
        ),
        observer,
        list,
        ids,
        _inbox: receiver,
    }
}

fn offered(harness: &Harness, index: usize) -> Arc<Track> {
    Arc::clone(&harness.actor.offers[index].track)
}

fn stranger() -> Arc<Track> {
    track("sine.flac", "stranger")
}

// ---- is_active and refill ----

#[test]
#[cfg_attr(miri, ignore = "decodes files, which Miri's isolation forbids")]
fn a_stopped_player_is_not_active_and_does_not_fill_the_slot() {
    let mut harness = harness(&["sine.flac", "sine.flac"]);

    assert!(!harness.actor.is_active());
    harness.actor.refill();

    assert!(harness.actor.offers.is_empty());
    assert_eq!(harness.actor.in_slot, None);
    assert_eq!(harness.calls(), 0);
}

#[test]
#[cfg_attr(miri, ignore = "decodes files, which Miri's isolation forbids")]
fn a_player_is_active_when_something_plays_or_has_been_taken() {
    let mut harness = harness(&["sine.flac", "sine.flac"]);
    let offer = |position| Offer {
        position,
        track: stranger(),
    };

    harness.actor.taken.push_back(offer(harness.at(0)));
    assert!(harness.actor.is_active());
    harness.actor.taken.clear();
    assert!(!harness.actor.is_active());
    harness.actor.playing = Some(offer(harness.at(0)));
    assert!(harness.actor.is_active());
}

#[test]
#[cfg_attr(miri, ignore = "decodes files, which Miri's isolation forbids")]
fn starting_a_track_offers_the_next_one_and_refilling_again_offers_nothing_new() {
    let mut harness = harness(&["sine.flac", "sine.flac", "sine.flac"]);

    harness.play();
    assert_eq!(harness.actor.offers.len(), 1);
    assert_eq!(harness.actor.in_slot, Some(harness.at(1)));
    harness.actor.refill();

    assert_eq!(harness.actor.offers.len(), 1);
}

#[test]
#[cfg_attr(miri, ignore = "decodes files, which Miri's isolation forbids")]
fn a_changed_answer_is_offered_and_clearing_it_empties_the_slot() {
    let mut harness = harness(&["sine.flac", "sine.flac", "sine.flac"]);
    harness.play();

    let (playlist, entry) = (harness.list, harness.ids[2]);
    harness
        .ask(|reply| Command::Enqueue {
            playlist,
            entry,
            reply,
        })
        .expect("exists");
    assert_eq!(harness.actor.in_slot, Some(harness.at(2)));
    assert_eq!(harness.actor.offers.len(), 2);

    harness
        .ask(|reply| Command::SetStopAfterCurrent { stop: true, reply })
        .expect("answered");
    assert_eq!(harness.actor.in_slot, None);
    assert_eq!(harness.actor.offers.len(), 2);
}

#[test]
#[cfg_attr(miri, ignore = "decodes files, which Miri's isolation forbids")]
fn unreported_offers_are_kept_up_to_a_limit_and_the_oldest_go_first() {
    let mut harness = harness(&["sine.flac", "sine.flac", "sine.flac"]);
    harness.play();
    let (x, y) = (harness.at(1), harness.at(2));

    for round in 0..(UNREPORTED_OFFERS + 4) {
        let entry = if round % 2 == 0 { y } else { x };
        harness.actor.sequence.enqueue(entry).expect("exists");
        harness.actor.refill();
        harness.actor.sequence.take(entry);
        assert_eq!(
            harness.actor.offers.len(),
            (round + 2).min(UNREPORTED_OFFERS),
            "round {round}"
        );
    }
}

// ---- events ----

#[test]
#[cfg_attr(miri, ignore = "decodes files, which Miri's isolation forbids")]
fn taking_a_slot_moves_the_sequence_and_a_stranger_is_ignored() {
    let mut harness = harness(&["sine.flac", "sine.flac", "sine.flac"]);
    harness.play();
    let slot = offered(&harness, 0);

    harness.actor.on_event(EngineEvent::TookNext(stranger()));
    assert_eq!(harness.actor.offers.len(), 1);
    assert!(harness.actor.taken.is_empty());

    harness.actor.on_event(EngineEvent::TookNext(slot));
    assert_eq!(harness.actor.sequence.current(), Some(harness.at(1)));
    assert_eq!(harness.actor.taken.len(), 1);
    assert_eq!(harness.actor.in_slot, Some(harness.at(2)));
    assert_eq!(harness.actor.offers.len(), 1);
}

#[test]
#[cfg_attr(miri, ignore = "decodes files, which Miri's isolation forbids")]
fn hearing_a_taken_track_makes_it_the_current_one_and_clears_the_failure_count() {
    let mut harness = harness(&["sine.flac", "sine.flac", "sine.flac"]);
    harness.play();
    let slot = offered(&harness, 0);
    harness
        .actor
        .on_event(EngineEvent::TookNext(Arc::clone(&slot)));
    harness.actor.failures = 3;

    harness.actor.on_event(EngineEvent::NowPlaying(stranger()));
    assert_eq!(harness.actor.failures, 3);
    assert_eq!(
        harness.actor.playing.as_ref().map(|o| o.position),
        Some(harness.at(0))
    );

    harness.actor.on_event(EngineEvent::NowPlaying(slot));
    assert_eq!(harness.actor.failures, 0);
    assert_eq!(
        harness.actor.playing.as_ref().map(|o| o.position),
        Some(harness.at(1))
    );
    assert!(harness.actor.taken.is_empty());
}

#[test]
#[cfg_attr(miri, ignore = "decodes files, which Miri's isolation forbids")]
fn a_slot_that_failed_to_open_is_passed_over_and_counted() {
    let mut harness = harness(&["sine.flac", "missing.flac", "sine.flac"]);
    harness.play();
    let slot = offered(&harness, 0);

    harness.actor.on_event(EngineEvent::Failed(slot));

    assert_eq!(harness.actor.failures, 1);
    assert_eq!(harness.actor.sequence.current(), Some(harness.at(1)));
    assert!(harness.actor.offers.is_empty());
    assert_eq!(harness.actor.in_slot, None);
}

#[test]
#[cfg_attr(miri, ignore = "decodes files, which Miri's isolation forbids")]
fn a_failure_while_decoding_a_track_being_heard_or_taken_is_counted() {
    let mut harness = harness(&["sine.flac", "sine.flac", "sine.flac"]);
    harness.play();
    let playing = Arc::clone(&harness.actor.playing.as_ref().expect("playing").track);
    let slot = offered(&harness, 0);
    harness
        .actor
        .on_event(EngineEvent::TookNext(Arc::clone(&slot)));

    harness.actor.on_event(EngineEvent::Failed(playing));
    assert_eq!(harness.actor.failures, 1);
    harness.actor.on_event(EngineEvent::Failed(slot));
    assert_eq!(harness.actor.failures, 2);
}

#[test]
#[cfg_attr(miri, ignore = "decodes files, which Miri's isolation forbids")]
fn a_failure_event_about_a_track_it_does_not_know_is_ignored() {
    let mut harness = harness(&["sine.flac", "sine.flac"]);
    harness.play();

    harness.actor.on_event(EngineEvent::Failed(stranger()));

    assert_eq!(harness.actor.failures, 0);
    assert_eq!(harness.actor.offers.len(), 1);
}

// ---- the end of playback ----

#[test]
#[cfg_attr(miri, ignore = "decodes files, which Miri's isolation forbids")]
fn an_end_event_with_nothing_playing_is_ignored_even_if_the_sequence_has_a_next() {
    let mut harness = harness(&["sine.flac", "sine.flac"]);
    let before = harness.calls();

    harness.actor.on_event(EngineEvent::Ended);

    assert!(harness.actor.playing.is_none());
    assert_eq!(harness.calls(), before);
}

#[test]
#[cfg_attr(miri, ignore = "decodes files, which Miri's isolation forbids")]
fn an_end_event_while_the_engine_is_playing_is_stale_and_ignored() {
    let mut harness = harness(&["sine.flac", "sine.flac"]);
    harness.play();

    harness.actor.on_event(EngineEvent::Ended);

    assert_eq!(
        harness.actor.playing.as_ref().map(|o| o.position),
        Some(harness.at(0))
    );
    assert_eq!(harness.actor.offers.len(), 1);
}

#[test]
#[cfg_attr(miri, ignore = "decodes files, which Miri's isolation forbids")]
fn when_playback_ended_and_the_sequence_has_a_next_it_is_started() {
    let mut harness = harness(&["sine.flac", "sine.flac"]);
    harness.play();
    block_on(harness.actor.engine.stop()).expect("the engine stops");

    harness.actor.on_event(EngineEvent::Ended);

    assert_eq!(
        harness.actor.playing.as_ref().map(|o| o.position),
        Some(harness.at(1))
    );
}

#[test]
#[cfg_attr(miri, ignore = "decodes files, which Miri's isolation forbids")]
fn when_playback_ended_with_stop_after_current_it_stays_stopped_and_the_flag_clears() {
    let mut harness = harness(&["sine.flac", "sine.flac"]);
    harness.play();
    harness.actor.sequence.set_stop_after_current(true);
    block_on(harness.actor.engine.stop()).expect("the engine stops");
    harness.actor.failures = 2;

    harness.actor.on_event(EngineEvent::Ended);

    assert!(harness.actor.playing.is_none());
    assert!(!harness.actor.sequence.stop_after_current());
    assert_eq!(harness.actor.failures, 0);
}

// ---- starting a track ----

#[test]
#[cfg_attr(miri, ignore = "decodes files, which Miri's isolation forbids")]
fn a_jump_does_not_consume_the_queue_but_an_ordinary_start_does() {
    let mut harness = harness(&["sine.flac", "sine.flac"]);
    harness
        .actor
        .sequence
        .enqueue(harness.at(0))
        .expect("exists");

    harness.actor.begin(harness.at(0), true).expect("plays");
    assert_eq!(harness.actor.sequence.queue().len(), 1);
    harness.actor.begin(harness.at(0), false).expect("plays");
    assert_eq!(harness.actor.sequence.queue().len(), 0);
}

#[test]
#[cfg_attr(miri, ignore = "decodes files, which Miri's isolation forbids")]
fn starting_a_track_that_is_gone_is_an_error() {
    let mut harness = harness(&["sine.flac"]);
    let gone = harness.at(0);
    harness
        .actor
        .sequence
        .remove(harness.list, &[harness.ids[0]])
        .expect("exists");

    let result = harness.actor.begin(gone, false);

    assert!(
        matches!(result, Err(PlayerError::Sequence(_))),
        "{result:?}"
    );
}
