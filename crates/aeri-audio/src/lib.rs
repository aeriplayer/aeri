//! Audio output: the [`OutputSink`] trait the engine writes to, and its implementations.
//!
//! The only crate permitted to contain `unsafe`; see `docs/RUST_STYLE.md` §7. Each use opts in
//! on its item (ADR 0016).
//!
//! The `test-util` feature adds [`FakeSink`], an in-memory sink for tests of code that
//! drives one. It is for dev-dependencies only.

#[cfg(test)]
mod alloc_guard;
mod bridge;
#[cfg(target_os = "macos")]
mod coreaudio;
#[cfg(feature = "test-util")]
mod fake;
#[cfg(target_os = "linux")]
mod pipewire;
mod sink;
mod sync;

#[cfg(target_os = "macos")]
pub use coreaudio::CoreAudioSink;
#[cfg(target_os = "linux")]
pub use pipewire::PipeWireSink;

#[cfg(feature = "test-util")]
pub use fake::{FakeSink, FakeSinkObserver, SinkCall};
pub use sink::{OutputSink, SinkError};
