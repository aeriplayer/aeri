//! Audio output: the [`OutputSink`] trait the engine writes to, and its implementations.
//!
//! The only crate permitted to contain `unsafe`; see `docs/RUST_STYLE.md` §7.
//!
//! The `test-util` feature adds [`FakeSink`], an in-memory sink for tests of code that
//! drives one. It is for dev-dependencies only.

#[cfg(feature = "test-util")]
mod fake;
mod sink;

#[cfg(feature = "test-util")]
pub use fake::{FakeSink, FakeSinkObserver, SinkCall};
pub use sink::{OutputSink, SinkError};
