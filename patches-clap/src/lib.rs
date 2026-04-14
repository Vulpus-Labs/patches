//! CLAP audio plugin wrapping the Patches engine.
//!
//! This crate builds as a `cdylib` exporting the CLAP entry point.
//! The host calls into the CLAP C API; this crate translates those
//! calls into [`PatchProcessor`](patches_engine::PatchProcessor)
//! operations.
//!
//! **clap-wrapper compatibility**: this plugin restricts itself to
//! CLAP features that have VST3 equivalents, so that `clap-wrapper`
//! can wrap it as a VST3 without loss of functionality.

pub mod descriptor;
pub mod diagnostic_widget;
pub mod entry;
pub mod error;
pub mod extensions;
pub mod factory;
pub mod gui;
pub mod gui_vizia;
pub mod plugin;
