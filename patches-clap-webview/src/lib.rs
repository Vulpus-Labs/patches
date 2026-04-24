#![allow(clippy::result_large_err)]
//! CLAP audio plugin wrapping the Patches engine, with a wry-based
//! webview GUI.
//!
//! Sibling of `patches-clap-vizia`. The descriptor id is distinct so
//! both plugins can coexist in a single host scan. For ticket 0670 the
//! webview only renders a static "hello" HTML page — IPC and real UI
//! content come in 0671/0672.

pub mod descriptor;
pub mod entry;
pub mod error;
pub mod extensions;
pub mod factory;
pub mod gui_webview;
pub mod plugin;

pub use patches_plugin_common::{DiagnosticView, GuiState, STATUS_LOG_CAPACITY};
