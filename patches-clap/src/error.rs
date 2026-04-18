//! Re-export of the shared `CompileError` from `patches-host`.
//!
//! Kept as a module so existing call sites (`crate::error::CompileError`)
//! continue to compile after the host-crate consolidation (ticket 0518).
pub use patches_host::CompileError;
