//! Node ids and constructors.

use std::collections::BTreeMap;

use crate::chain::Chain;
use crate::error::DspError;
use crate::node::DspNode;

/// Builds a fresh node.
pub type NodeConstructor = fn() -> Box<dyn DspNode>;

/// Maps node ids to constructors, so a chain can be built from a list of names.
#[derive(Debug, Default, Clone)]
pub struct Registry {
    constructors: BTreeMap<&'static str, NodeConstructor>,
}

impl Registry {
    /// A registry with no nodes.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `constructor` under `id`.
    ///
    /// # Errors
    ///
    /// [`DspError::DuplicateNode`] if `id` is already registered; the registry is unchanged.
    pub fn register(
        &mut self,
        id: &'static str,
        constructor: NodeConstructor,
    ) -> Result<(), DspError> {
        match self.constructors.entry(id) {
            std::collections::btree_map::Entry::Occupied(_) => Err(DspError::DuplicateNode { id }),
            std::collections::btree_map::Entry::Vacant(slot) => {
                slot.insert(constructor);
                Ok(())
            }
        }
    }

    /// Builds a chain of new nodes, one per id, in the order given.
    ///
    /// # Errors
    ///
    /// [`DspError::UnknownNode`] for the first id that is not registered.
    pub fn build(&self, ids: &[&str]) -> Result<Chain, DspError> {
        let mut chain = Chain::new();
        for &id in ids {
            let constructor = self
                .constructors
                .get(id)
                .ok_or_else(|| DspError::UnknownNode { id: id.to_owned() })?;
            chain.push(constructor());
        }
        Ok(chain)
    }
}

#[cfg(test)]
mod tests {
    use aeri_core::Channels;

    use super::*;
    use crate::node::{Control, TrackStart};

    struct Adder(f32);

    impl DspNode for Adder {
        fn start_track(&mut self, _: &TrackStart) {}
        fn control(&mut self, _: &Control) {}
        fn process(&mut self, interleaved: &mut [f32]) {
            for sample in interleaved {
                *sample += self.0;
            }
        }
        fn reset(&mut self) {}
    }

    fn one() -> Box<dyn DspNode> {
        Box::new(Adder(1.0))
    }

    fn ten() -> Box<dyn DspNode> {
        Box::new(Adder(10.0))
    }

    fn registry() -> Registry {
        let mut registry = Registry::new();
        registry.register("one", one).expect("first registration");
        registry.register("ten", ten).expect("different id");
        registry
    }

    fn run(chain: &mut Chain, sample: f32) -> f32 {
        let mut block = [sample];
        chain.process(&mut block, Channels::new(1).expect("non-zero"));
        block[0]
    }

    #[test]
    fn build_makes_one_node_per_id_in_the_order_given() {
        let mut chain = registry()
            .build(&["ten", "one", "ten"])
            .expect("all ids are registered");
        assert!((run(&mut chain, 0.0) - 21.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_built_chain_forwards_every_call() {
        let mut chain = registry().build(&["one"]).expect("registered");
        let spec = aeri_core::StreamSpec {
            sample_rate: aeri_core::SampleRate::new(48_000).expect("non-zero"),
            channels: Channels::new(1).expect("non-zero"),
        };
        chain.start_track(&TrackStart::new(spec, aeri_core::ReplayGain::default()));
        chain.control(&Control::Volume(aeri_core::Volume::UNITY));
        chain.reset();
        assert!((run(&mut chain, 0.0) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn an_empty_list_builds_an_empty_chain() {
        let mut chain = registry().build(&[]).expect("nothing to look up");
        assert!((run(&mut chain, 0.5) - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn an_unknown_id_names_itself() {
        let err = registry()
            .build(&["one", "missing"])
            .expect_err("`missing` is not registered");
        assert_eq!(
            err,
            DspError::UnknownNode {
                id: "missing".into()
            }
        );
        assert_eq!(err.to_string(), "no DSP node is registered as `missing`");
    }

    #[test]
    fn registering_an_id_twice_fails_and_keeps_the_first() {
        let mut registry = registry();
        let err = registry.register("one", ten).expect_err("`one` is taken");
        assert_eq!(err, DspError::DuplicateNode { id: "one" });
        assert_eq!(err.to_string(), "a DSP node is already registered as `one`");

        let mut chain = registry.build(&["one"]).expect("still registered");
        assert!((run(&mut chain, 0.0) - 1.0).abs() < f32::EPSILON);
    }
}
