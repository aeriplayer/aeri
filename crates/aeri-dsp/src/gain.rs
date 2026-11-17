//! A gain that ramps to a new target across one block.

/// A linear gain, applied to interleaved frames.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Gain {
    current: f32,
    target: f32,
}

#[expect(
    clippy::cast_precision_loss,
    reason = "blocks are far below 2^24 frames"
)]
fn fraction(done: usize, total: usize) -> f32 {
    done as f32 / total as f32
}

impl Gain {
    pub(crate) const UNITY: Self = Self {
        current: 1.0,
        target: 1.0,
    };

    /// The next block ramps to `target`.
    pub(crate) fn ramp_to(&mut self, target: f32) {
        self.target = target;
    }

    /// The gain is `target` from the next sample, with no ramp.
    pub(crate) fn snap_to(&mut self, target: f32) {
        self.current = target;
        self.target = target;
    }

    /// Drops any ramp in progress: the gain is its target from the next sample.
    pub(crate) fn settle(&mut self) {
        self.current = self.target;
    }

    /// Multiplies each whole frame of `width` samples; at exactly unity the block is not written.
    pub(crate) fn apply(&mut self, block: &mut [f32], width: usize) {
        if self.current.to_bits() == self.target.to_bits() {
            if self.target.to_bits() != 1.0_f32.to_bits() {
                for sample in block {
                    *sample *= self.target;
                }
            }
            return;
        }

        let frames = block.len() / width;
        let (from, to) = (self.current, self.target);
        let (low, high) = (from.min(to), from.max(to));
        for (index, frame) in block.chunks_exact_mut(width).enumerate() {
            let gain = if index + 1 == frames {
                to
            } else {
                (to - from)
                    .mul_add(fraction(index + 1, frames), from)
                    .clamp(low, high)
            };
            for sample in frame {
                *sample *= gain;
            }
        }
        self.current = to;
    }
}
