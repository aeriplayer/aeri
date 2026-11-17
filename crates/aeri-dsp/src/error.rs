//! Errors from building a chain.

/// Why a registry could not register a node or build a chain.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DspError {
    /// A chain named an id that no node was registered under.
    #[error("no DSP node is registered as `{id}`")]
    UnknownNode {
        /// The id that was asked for.
        id: String,
    },
    /// Two nodes were registered under one id.
    #[error("a DSP node is already registered as `{id}`")]
    DuplicateNode {
        /// The id that was registered twice.
        id: &'static str,
    },
}
