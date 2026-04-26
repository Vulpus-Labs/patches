//! GUI shared state.
//!
//! The [`GuiState`] struct is shared between the plugin (main thread)
//! and the embedded GUI window (vizia/baseview or webview) via
//! `Arc<Mutex<GuiState>>`.

use std::collections::VecDeque;
use std::path::PathBuf;

use patches_core::source_map::SourceMap;
use patches_diagnostics::{source_line_col, RenderedDiagnostic, Severity};
use patches_engine::HaltInfoSnapshot;
use serde::{Deserialize, Serialize};

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
#[derive(Default, Serialize)]
pub struct GuiState {
    /// Currently loaded file path (displayed in the UI).
    pub file_path: Option<PathBuf>,
    /// Set to true by the Browse button; consumed by `on_main_thread`.
    pub browse_requested: bool,
    /// Set to true by the Reload button; consumed by `on_main_thread`.
    pub reload_requested: bool,
    /// Mirror of the plugin's persisted module search paths. Edited from
    /// the GUI; written back to `PatchesClapPlugin::module_paths` by
    /// `on_main_thread`. Changes do not auto-rescan — the user must press
    /// the Rescan button for the new paths to take effect.
    pub module_paths: Vec<PathBuf>,
    /// Set to true by the "Add path" button; consumed by `on_main_thread`,
    /// which opens a directory picker.
    pub add_path_requested: bool,
    /// Index of a module path to remove, set by a per-row delete button.
    pub remove_path_index: Option<usize>,
    /// Set to true by the Rescan button; triggers the hard-stop reload
    /// flow (ADR 0044 §3).
    pub rescan_requested: bool,
    /// Set to true by the webview's meter poll; consumed by `on_main_thread`
    /// which reads the MeterTap and pushes a frame to JS. Not part of the
    /// snapshot.
    #[serde(skip)]
    pub meter_poll_requested: bool,
    /// Rolling log of the most recent status messages (newest last).
    pub status_log: VecDeque<String>,
    /// Current diagnostics plus the source map needed to render them.
    /// Skipped in the default serialisation — webview shells project
    /// these through a dedicated channel.
    #[serde(skip)]
    pub diagnostic_view: DiagnosticView,
    /// Engine halt state (ADR 0051). `Some(_)` triggers a top-of-window
    /// error banner; cleared by the audio callback once the rebuilt engine
    /// reports no halt. Skipped in the default serialisation.
    #[serde(skip)]
    pub halt: Option<HaltInfoSnapshot>,
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

/// Versioned snapshot of [`GuiState`] projected to a shape a webview can
/// consume. Ticket 0671.
///
/// Keep the field set small and string-typed — the JS side is hand-written.
/// Bump `v` whenever the shape changes in a breaking way.
#[derive(Serialize, PartialEq, Eq)]
pub struct GuiSnapshot {
    pub v: u32,
    pub file_path: Option<String>,
    pub module_paths: Vec<String>,
    pub status_log: Vec<String>,
    pub browse_requested: bool,
    pub reload_requested: bool,
    pub add_path_requested: bool,
    pub rescan_requested: bool,
    pub remove_path_index: Option<usize>,
    pub halt_message: Option<String>,
    pub diagnostics: Vec<DiagnosticSummary>,
    /// Raw text of the loaded `.patches` file, if readable. Spike: the
    /// webview Source tab renders this with Shiki. None when no file is
    /// loaded or the file can't be read.
    pub patch_source: Option<String>,
}

/// Compact, webview-facing projection of a [`RenderedDiagnostic`].
/// Drops the snippet highlighting — consumers render severity + message
/// + location only.
#[derive(Serialize, PartialEq, Eq)]
pub struct DiagnosticSummary {
    pub severity: &'static str,
    pub code: Option<String>,
    pub message: String,
    /// `file:line:col` of the primary snippet, when a source map is present.
    pub location: Option<String>,
    pub label: String,
}

impl GuiSnapshot {
    pub const VERSION: u32 = 3;

    /// Project a `GuiState` into the webview-facing shape.
    pub fn from_state(state: &GuiState) -> Self {
        Self {
            v: Self::VERSION,
            file_path: state
                .file_path
                .as_ref()
                .map(|p| p.display().to_string()),
            module_paths: state
                .module_paths
                .iter()
                .map(|p| p.display().to_string())
                .collect(),
            status_log: state.status_log.iter().cloned().collect(),
            browse_requested: state.browse_requested,
            reload_requested: state.reload_requested,
            add_path_requested: state.add_path_requested,
            rescan_requested: state.rescan_requested,
            remove_path_index: state.remove_path_index,
            halt_message: state.halt.as_ref().map(format_halt),
            diagnostics: summarise_diagnostics(&state.diagnostic_view),
            patch_source: state
                .file_path
                .as_ref()
                .and_then(|p| std::fs::read_to_string(p).ok()),
        }
    }
}

fn summarise_diagnostics(view: &DiagnosticView) -> Vec<DiagnosticSummary> {
    view.diagnostics
        .iter()
        .map(|d| DiagnosticSummary {
            severity: severity_str(d.severity),
            code: d.code.clone(),
            message: d.message.clone(),
            location: view.source_map.as_ref().map(|map| {
                let (line, col) = source_line_col(map, d.primary.source, d.primary.range.start);
                let path = map
                    .path(d.primary.source)
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| format!("<source#{}>", d.primary.source.0));
                format!("{path}:{line}:{col}")
            }),
            label: d.primary.label.clone(),
        })
        .collect()
}

fn severity_str(s: Severity) -> &'static str {
    match s {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Note => "note",
    }
}

fn format_halt(h: &HaltInfoSnapshot) -> String {
    let first = h.payload.lines().next().unwrap_or("");
    format!(
        "Engine halted: module {:?} (slot {}) panicked: {}",
        h.module_name, h.slot, first,
    )
}

/// Intents posted by the webview via `window.ipc.postMessage(JSON)`.
/// Tagged by a `kind` discriminator matching the `*_requested` flags in
/// [`GuiState`]. Ticket 0671.
#[derive(Deserialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Intent {
    Browse,
    Reload,
    Rescan,
    AddPath,
    RemovePath { index: usize },
    /// Webview asks for a meter frame. Handled directly by the GUI shell,
    /// not via `GuiState` — kept in this enum for a single JSON surface.
    PollMeter,
}

impl Intent {
    /// Flip the corresponding flag(s) on `GuiState`. The plugin's
    /// `on_main_thread` consumes these on the next tick.
    pub fn apply(self, state: &mut GuiState) {
        match self {
            Intent::Browse => state.browse_requested = true,
            Intent::Reload => state.reload_requested = true,
            Intent::Rescan => state.rescan_requested = true,
            Intent::AddPath => state.add_path_requested = true,
            Intent::RemovePath { index } => state.remove_path_index = Some(index),
            Intent::PollMeter => state.meter_poll_requested = true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Intent {
        serde_json::from_str(json).expect("intent json")
    }

    #[test]
    fn reload_intent_flips_flag() {
        let mut g = GuiState::default();
        parse(r#"{"kind":"reload"}"#).apply(&mut g);
        assert!(g.reload_requested);
    }

    #[test]
    fn all_simple_intents_roundtrip() {
        let mut g = GuiState::default();
        parse(r#"{"kind":"browse"}"#).apply(&mut g);
        parse(r#"{"kind":"rescan"}"#).apply(&mut g);
        parse(r#"{"kind":"add_path"}"#).apply(&mut g);
        parse(r#"{"kind":"remove_path","index":2}"#).apply(&mut g);
        assert!(g.browse_requested);
        assert!(g.rescan_requested);
        assert!(g.add_path_requested);
        assert_eq!(g.remove_path_index, Some(2));
    }

    #[test]
    fn snapshot_versioned_and_projects_fields() {
        let mut g = GuiState::default();
        g.push_status("hello");
        g.module_paths.push("/tmp/a".into());
        let snap = GuiSnapshot::from_state(&g);
        assert_eq!(snap.v, GuiSnapshot::VERSION);
        assert_eq!(snap.status_log, vec!["hello".to_string()]);
        assert_eq!(snap.module_paths, vec!["/tmp/a".to_string()]);
        let json = serde_json::to_string(&snap).unwrap();
        assert!(json.contains("\"v\":3"));
    }
}
