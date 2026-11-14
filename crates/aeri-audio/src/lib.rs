// aeri-audio is the only crate permitted to use unsafe — FFI and the real-time
// SPSC bridge require it. Every unsafe block must carry a `// SAFETY:` comment.
#![allow(unsafe_code)]
//! Audio output: `OutputSink` trait, native CoreAudio/PipeWire backends, real-time thread.
