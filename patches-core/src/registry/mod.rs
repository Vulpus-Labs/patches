//! Module registration, lookup, and plugin loading surface for the
//! Patches kernel.
//!
//! Previously lived in its own `patches-registry` crate (ADR 0040).
//! [ADR 0073](../../../../adr/0073-monorepo-split-into-successor-repos.md)
//! folds the registry back into `patches-core` so the published-crate
//! surface collapses to three (patches-sdk, patches-core,
//! patches-ffi-common).

pub mod module_builder;
#[allow(clippy::module_inception)]
pub mod registry;

pub use module_builder::{Builder, ModuleBuilder};
pub use registry::{Registry, RegisterOutcome};
