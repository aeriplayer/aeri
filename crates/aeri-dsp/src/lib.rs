//! The DSP chain between the decoder and the sink ([ADR 0020](../../docs/adr/0020-aeri-dsp-in-place-interleaved-chain.md)).
//!
//! A [`DspNode`] processes interleaved `f32` frames in place and never changes the stream
//! spec. A [`Chain`] runs nodes in order over pieces of at most [`BLOCK_FRAMES`] frames, and a
//! [`Registry`] builds a chain from node ids ([ADR 0002](../../docs/adr/0002-compiled-in-extension-points.md)).

mod chain;
mod error;
mod node;
#[cfg(test)]
mod recorder;
mod registry;

pub use chain::{BLOCK_FRAMES, Chain};
pub use error::DspError;
pub use node::{Control, DspNode, TrackStart};
pub use registry::{NodeConstructor, Registry};
