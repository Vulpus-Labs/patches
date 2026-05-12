//! `patches-sdk` — supported surface for external module authors.
//!
//! This crate is the single dependency a Patches module bundle adds
//! via `cargo add patches-sdk`. It re-exports the Module trait,
//! descriptors, ports, cables, parameters, and the
//! [`export_modules!`] macro that emits the eight ABI entry points
//! per module type.
//!
//! ```toml
//! [dependencies]
//! patches-sdk = "0.7"
//!
//! # Optional, for module authors who want DSP kernels or FFT harness:
//! patches-dsp = { git = "https://github.com/.../patches", tag = "v0.7.0" }
//! ```
//!
//! ## Contract
//!
//! Anything reachable from this crate's public API is supported. If
//! you need a `patches-core` type, helper, or macro that does not
//! appear here, file an issue — the SDK surface gets deliberate
//! extensions, not unannounced direct dependencies on the underlying
//! foundation crates.
//!
//! `patches-dsp` is intentionally **not** re-exported. Kernel
//! authors who want SVF, ladder, biquad, ADSR, etc. depend on
//! `patches-dsp` themselves (git dep today; crates.io later).
//!
//! ## Layout
//!
//! - Module trait, descriptors, instance plumbing → flat at crate
//!   root (see [`Module`], [`AudioEnvironment`], [`BuildError`]).
//! - Registry types → [`registry`].
//! - Submodules ([`cables`], [`modules`], [`param_frame`],
//!   [`parameter_map`], [`build_error`], [`module_params`],
//!   [`cable_pool`]) re-exported so existing call sites
//!   (`use patches_sdk::cables::TriggerInput;`) keep working.
//! - `export_modules!` macro available at crate root.
//! - `test_support` re-exported under the `test-support` feature.

#![warn(missing_docs)]

// ── patches-core re-exports ──────────────────────────────────────────────────

#[doc(inline)]
pub use patches_core::*;

/// Module registration, lookup, and plugin loading surface.
pub use patches_core::registry;

/// Cable kinds and port-binding types passed between modules.
pub use patches_core::cables;

/// Module descriptor / template types.
pub use patches_core::modules;

/// Parameter frame view and helpers.
pub use patches_core::param_frame;

/// Parameter map types used at instantiation.
pub use patches_core::parameter_map;

/// Build-error type returned by `Module::build` / `Module::prepare`.
pub use patches_core::build_error;

/// Cable pool — audio-thread scratch + cycle slot accessors.
pub use patches_core::cable_pool;

/// Parameter-template macro helpers used by `ModuleDescriptorTemplate`.
pub use patches_core::module_params;

/// Test-only harness (feature `test-support`).
#[cfg(feature = "test-support")]
pub use patches_core::test_support;

// ── patches-ffi-common re-exports ────────────────────────────────────────────

/// FFI bundle entry-point macro.
///
/// One invocation emits the eight ABI entry points per module type,
/// a combined vtable array, a `patches_plugin_init` symbol the host
/// reads, and a `patches_plugin_descriptor_hash_<Name>` symbol per
/// module.
pub use patches_ffi_common::export_modules;

/// Single-module variant of [`export_modules!`] kept for tests and
/// legacy plugins that emit one module per crate.
pub use patches_ffi_common::export_plugin;

/// Hash-override variant used by the bad-hash compatibility tests.
pub use patches_ffi_common::export_plugin_with_hash_override;

/// ABI types (`ABI_VERSION`, `FfiBytes`, `FfiPluginManifest`, etc.).
///
/// Bundle authors should not need these for normal module
/// authorship — they are exposed for bundle-side
/// `patches_plugin_init` round-trip tests.
pub use patches_ffi_common::types;

/// Descriptor-template JSON helpers used by bundle round-trip tests.
pub use patches_ffi_common::json;

/// Low-level ABI types used by hand-rolled `export_plugin!` plugins
/// (legacy single-module bundles and the test-plugins crates). Most
/// module bundles should not need this — prefer [`export_modules!`].
pub use patches_ffi_common::abi;

/// Port-frame layout types used by hand-rolled plugins to thread the
/// host port indices into per-module state.
pub use patches_ffi_common::port_frame;

/// Plugin-side scaffolding types (`decode_param_frame`,
/// `PluginInstance`, `prepare_dispatch`). Used by hand-rolled
/// `export_plugin!` plugins; `export_modules!` hides these.
pub use patches_ffi_common::sdk;

/// Descriptor-hash helper, used by `export_plugin_with_hash_override!`
/// (bad-hash test plugin) to fabricate a deliberately incorrect hash.
pub use patches_ffi_common::descriptor_hash;
