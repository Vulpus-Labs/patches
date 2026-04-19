//! `patches-registry` — module registration, lookup, and plugin loading
//! surface for the Patches kernel.
//!
//! Split out of `patches-core` per ADR 0040 so consumers that need
//! registration or module lookup (planner, interpreter, LSP, FFI plugin
//! loaders) depend on this crate directly without dragging registry-agnostic
//! core types through it.

pub mod file_processor;
pub mod module_builder;
pub mod registry;

pub use file_processor::FileProcessor;
pub use module_builder::{Builder, ModuleBuilder};
pub use registry::{Registry, RegisterOutcome};
