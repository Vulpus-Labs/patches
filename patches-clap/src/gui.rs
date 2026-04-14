//! GUI shared state.
//!
//! The [`GuiState`] struct is shared between the plugin (main thread)
//! and the embedded GUI window (vizia/baseview thread) via
//! `Arc<Mutex<GuiState>>`.

use std::collections::VecDeque;
use std::path::PathBuf;

use patches_core::source_map::SourceMap;
use patches_diagnostics::RenderedDiagnostic;

/// Upper bound on retained status messages. Older entries drop off the
/// front when the log grows past this size.
pub const STATUS_LOG_CAPACITY: usize = 100;

/// Structured diagnostics from the most recent compile attempt, paired with
/// the source map used to resolve their spans. Cleared on successful compile.
#[derive(Clone, Default)]
pub struct DiagnosticView {
    pub diagnostics: Vec<RenderedDiagnostic>,
    pub source_map: Option<SourceMap>,
}

/// Shared state between the plugin and the embedded GUI.
#[derive(Default)]
pub struct GuiState {
    /// Currently loaded file path (displayed in the UI).
    pub file_path: Option<PathBuf>,
    /// Set to true by the Browse button; consumed by `on_main_thread`.
    pub browse_requested: bool,
    /// Set to true by the Reload button; consumed by `on_main_thread`.
    pub reload_requested: bool,
    /// Rolling log of the most recent status messages (newest last).
    pub status_log: VecDeque<String>,
    /// Current diagnostics plus the source map needed to render them.
    pub diagnostic_view: DiagnosticView,
}

impl GuiState {
    /// Append a status message, evicting the oldest entries once the log
    /// reaches [`STATUS_LOG_CAPACITY`].
    pub fn push_status(&mut self, msg: impl Into<String>) {
        if self.status_log.len() >= STATUS_LOG_CAPACITY {
            self.status_log.pop_front();
        }
        self.status_log.push_back(msg.into());
    }

    /// Render the log as a single newline-joined string for display.
    pub fn status_text(&self) -> String {
        if self.status_log.is_empty() {
            String::new()
        } else {
            self.status_log
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        }
    }
}
