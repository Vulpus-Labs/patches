//! GUI-toolkit-agnostic state shared by Patches plugin crates.
//!
//! Today: the plain `GuiState` struct wired between the CLAP plugin
//! (main thread) and an embedded GUI (vizia or webview) via
//! `Arc<Mutex<GuiState>>`. Intentionally no `PluginGui` trait yet —
//! one implementation is not enough to design an abstraction around.

pub mod gui;
pub mod meter;

pub use gui::{DiagnosticView, GuiSnapshot, GuiState, Intent, STATUS_LOG_CAPACITY};
pub use meter::MeterTap;
