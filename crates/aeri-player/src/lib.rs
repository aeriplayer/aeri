//! Playlists, the queue, and playback order, and the [`Player`] handle that frontends use.
//!
//! [`Player`] is a cloneable handle to one thread that owns a [`Sequence`], which decides what
//! plays after what (ADR 0024), and the playback engine, whose slot it keeps filled
//! (ADR 0025). Frontends depend on this crate and on nothing beneath it (ADR 0006).
//!
//! The `test-util` feature adds [`Player::with_fake_sink`] and re-exports the fake sink it takes,
//! so frontends can test against a real player. It is for dev-dependencies only.

mod actor;
mod block_on;
mod error;
mod ids;
mod m3u;
mod message;
mod player;
mod playlist;
mod sequence;
mod shuffle;
mod state;

pub use aeri_engine::EXTENSIONS;
#[cfg(feature = "test-util")]
pub use aeri_engine::{FakeSink, FakeSinkObserver};
pub use error::{PlayerError, SequenceError};
pub use ids::{EntryId, PlaylistId, Position};
pub use m3u::parse_m3u;
pub use player::Player;
pub use playlist::{Entry, Playlist};
pub use sequence::{PlaybackOrder, Sequence};
pub use state::{PlayerState, Seek};
