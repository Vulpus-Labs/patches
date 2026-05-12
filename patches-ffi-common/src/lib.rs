//! Patches FFI common types and SDK helpers.
//!
//! The binary wire formats that cross this boundary —
//! [`param_frame`] scalars, [`port_frame::PortFrame`],
//! [`structural_frame`], and the [`patches_core::cables::CableValue`] cable
//! slot — are specified normatively in the manual at
//! `docs/src/abi/wire-formats.md`. External SDKs implementing the plugin
//! side of the contract should treat that page (not this crate's source)
//! as the spec; the Rust code here is the canonical *implementation*.
//!
//! The descriptor JSON shape that names the parameters and ports
//! referenced from the wire formats is at
//! `docs/src/abi/descriptor-schema.md`.

pub mod abi;
pub mod arc_table;
pub mod json;
pub mod port_frame;
pub mod sdk;
pub mod structural_frame;
#[cfg(test)]
mod sdk_fuzz;
pub mod types;

// Re-exports of the audio-thread parameter data plane, now owned by
// `patches-core` so the `Module` trait can name `ParamView` in its
// signature without a crate cycle.
pub use patches_core::param_frame;
pub use patches_core::param_layout;

// Re-exports used by `export_modules!`. The macro reaches every
// patches-core item it needs via `$crate::...` so a bundle can depend
// on this crate transitively (through `patches-sdk`) without listing
// `patches-core` directly in its Cargo.toml.
pub use patches_core::{Module, ModuleShape};
pub use patches_core::cable_pool;
pub use patches_core::cables;
pub use types::*;

/// Deterministic 64-bit hash over a [`patches_core::ModuleDescriptor`]'s
/// shape. Host and plugin compile units compute identical values; drift
/// triggers a load-time refusal (ADR 0045 §5, E104 ticket 0614).
pub fn descriptor_hash(descriptor: &patches_core::ModuleDescriptor) -> u64 {
    patches_core::param_layout::descriptor_hash(descriptor)
}
