#![allow(clippy::result_large_err)]
//! CLAP audio plugin wrapping the Patches engine, with a wry-based
//! webview GUI.

pub mod descriptor;
pub mod entry;
pub mod error;
pub mod extensions;
pub mod factory;
pub mod gui;
pub mod plugin;

pub use patches_plugin_common::{DiagnosticView, GuiState, STATUS_LOG_CAPACITY};
