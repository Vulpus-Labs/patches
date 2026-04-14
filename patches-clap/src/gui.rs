//! GUI shared state.
//!
//! The [`GuiState`] struct is shared between the plugin (main thread)
//! and the embedded GUI window (vizia/baseview thread) via
//! `Arc<Mutex<GuiState>>`.

use std::path::PathBuf;

/// Shared state between the plugin and the embedded GUI.
#[derive(Default)]
pub struct GuiState {
    /// Currently loaded file path (displayed in the UI).
    pub file_path: Option<PathBuf>,
    /// Set to true by the Browse button; consumed by `on_main_thread`.
    pub browse_requested: bool,
    /// Set to true by the Reload button; consumed by `on_main_thread`.
    pub reload_requested: bool,
    /// Status message shown in the UI (e.g. "Loaded", "Error: ...").
    pub status: String,
}
