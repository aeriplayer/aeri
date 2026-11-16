//! The bridge's shared-state primitives: `loom`'s under `cfg(loom)`, `std`'s otherwise (ADR 0018).

#[cfg(loom)]
pub(crate) use loom::sync::Arc;
#[cfg(loom)]
pub(crate) use loom::sync::atomic::{AtomicBool, AtomicU64, Ordering};
#[cfg(not(loom))]
pub(crate) use std::sync::Arc;
#[cfg(not(loom))]
pub(crate) use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
