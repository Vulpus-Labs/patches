//! Re-exports for the bundled module manifest (ADR 0066, ticket 0793).
//!
//! The JSON itself, parsing, and the describe-only registry construction
//! all live in `patches-manifest`. This module exists only so callers
//! within patches-lsp can reach those helpers via a stable local path.

pub use patches_manifest::bundled_manifest;
pub use patches_manifest::static_registry::registry_from_manifest;

#[cfg(test)]
pub(crate) fn manifest_registry() -> patches_core::registry::Registry {
    registry_from_manifest(bundled_manifest())
}
