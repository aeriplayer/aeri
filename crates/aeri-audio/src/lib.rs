//! Audio output: `OutputSink` trait, native CoreAudio/PipeWire backends, real-time thread.
//!
//! This is the only crate permitted to contain `unsafe`. The workspace denies
//! `unsafe_code`; when the first FFI call is written, opt in once at the crate root
//! with `#![expect(unsafe_code, reason = "...")]`.
