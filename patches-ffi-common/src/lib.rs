pub mod abi;
pub mod arc_table;
pub mod json;
pub mod port_frame;
pub mod types;

// Re-exports of the audio-thread parameter data plane, now owned by
// `patches-core` so the `Module` trait can name `ParamView` in its
// signature without a crate cycle.
pub use patches_core::ids;
pub use patches_core::ids::FloatBufferId;
pub use patches_core::param_frame;
pub use patches_core::param_layout;
pub use types::*;

/// Deterministic 64-bit hash over a [`patches_core::ModuleDescriptor`]'s
/// shape. Host and plugin compile units compute identical values; drift
/// triggers a load-time refusal (ADR 0045 §5, E104 ticket 0614).
pub fn descriptor_hash(descriptor: &patches_core::ModuleDescriptor) -> u64 {
    patches_core::param_layout::descriptor_hash(descriptor)
}
