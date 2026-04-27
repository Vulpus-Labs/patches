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

/// Compact projection of one `TapDescriptor` for the webview shell.
///
/// `kind` is `"compound"` for taps with two or more components, otherwise
/// the single component's name (e.g. `"meter"`, `"osc"`). `components`
/// preserves the full ordered list so the UI can pick a richer rendering
/// when more than one is present.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TapSummary {
    pub name: String,
    pub slot: usize,
    pub kind: String,
    pub components: Vec<String>,
}

/// Per-tap display configuration controlled by the webview. The
/// observer holds raw sample buffers; these values pick the
/// FFT size / decimation / window the next read uses.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TapDisplayOpts {
    pub spectrum_fft_size: usize,
    pub scope_decimation: usize,
    pub scope_window_samples: usize,
}

impl Default for TapDisplayOpts {
    fn default() -> Self {
        Self {
            spectrum_fft_size: 1024,
            scope_decimation: 16,
            scope_window_samples: 512,
        }
    }
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
    /// Tap manifest projection from the most recent successful compile,
    /// ordered by slot. Cleared on a failed compile and replaced on the
    /// next successful one.
    pub taps: Vec<TapSummary>,
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
    /// Per-slot display options (FFT size, scope decimation/window). The
    /// webview drives these via `Intent::SetTapOpts`; the plugin's
    /// `push_taps` reads them on each frame and forwards to
    /// `SubscribersHandle::read_*_into_with`.
    pub tap_opts: std::collections::HashMap<usize, TapDisplayOpts>,
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
    pub taps: Vec<TapSummary>,
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
    pub const VERSION: u32 = 4;

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
            taps: state.taps.clone(),
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

/// Per-slot live tap data pushed to the webview at frame rate, separate
/// from [`GuiSnapshot`] so it bypasses snapshot dedupe throttling.
///
/// Field names are deliberately short — every byte is serialised at
/// ~30 Hz. `w` (waveform) and `m` (magnitudes) are omitted when the tap
/// has no scope / spectrum component.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct TapSlotFrame {
    /// Tap slot index (matches `TapDescriptor::slot`).
    pub s: usize,
    /// Peak amplitude (linear).
    pub p: f32,
    /// RMS amplitude (linear).
    pub r: f32,
    /// Scope waveform samples (length `SCOPE_BUFFER_LEN` when present).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub w: Option<Vec<f32>>,
    /// Spectrum magnitudes (length `SPECTRUM_BIN_COUNT` when present).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub m: Option<Vec<f32>>,
    /// Gate LED scalar (0..1). Present only for taps with a `gate_led`
    /// component.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub g: Option<f32>,
    /// Trigger fired since last poll. `Some(true)` exactly when the
    /// observer's latching trigger cell held a non-zero value at read
    /// time (and was cleared by the read). Present only for taps with
    /// a `trigger_led` component. The visual flash + decay lives in
    /// the UI, since the audio side has no concept of UI cadence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub t: Option<bool>,
}

/// Versioned per-tick projection of live tap data. Bump [`TapFrame::VERSION`]
/// on breaking shape changes.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct TapFrame {
    pub v: u32,
    pub slots: Vec<TapSlotFrame>,
}

impl TapFrame {
    pub const VERSION: u32 = 1;

    pub fn new(slots: Vec<TapSlotFrame>) -> Self {
        Self { v: Self::VERSION, slots }
    }
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
    /// Update per-slot display options. Any field left `None` keeps
    /// its current value. Posted by the webview when the user picks
    /// an FFT size, decimation, or window length.
    SetTapOpts {
        slot: usize,
        spectrum_fft_size: Option<usize>,
        scope_decimation: Option<usize>,
        scope_window_samples: Option<usize>,
    },
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
            Intent::SetTapOpts {
                slot,
                spectrum_fft_size,
                scope_decimation,
                scope_window_samples,
            } => {
                let entry = state.tap_opts.entry(slot).or_insert_with(TapDisplayOpts::default);
                if let Some(n) = spectrum_fft_size { entry.spectrum_fft_size = n; }
                if let Some(d) = scope_decimation { entry.scope_decimation = d; }
                if let Some(w) = scope_window_samples { entry.scope_window_samples = w; }
            }
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
        assert_eq!(snap.v, 4);
        assert_eq!(snap.status_log, vec!["hello".to_string()]);
        assert_eq!(snap.module_paths, vec!["/tmp/a".to_string()]);
        let json = serde_json::to_string(&snap).unwrap();
        assert!(json.contains("\"v\":4"));
    }

    #[test]
    fn snapshot_carries_taps_in_slot_order() {
        let g = GuiState {
            taps: vec![
                TapSummary {
                    name: "kick".into(),
                    slot: 0,
                    kind: "meter".into(),
                    components: vec!["meter".into()],
                },
                TapSummary {
                    name: "snare".into(),
                    slot: 1,
                    kind: "compound".into(),
                    components: vec!["meter".into(), "osc".into()],
                },
            ],
            ..Default::default()
        };
        let snap = GuiSnapshot::from_state(&g);
        assert_eq!(snap.taps.len(), 2);
        assert_eq!(snap.taps[0].slot, 0);
        assert_eq!(snap.taps[1].kind, "compound");

        let json = serde_json::to_string(&snap).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let taps = parsed.get("taps").unwrap().as_array().unwrap();
        assert_eq!(taps.len(), 2);
        assert_eq!(taps[0].get("name").unwrap(), "kick");
        assert_eq!(taps[1].get("components").unwrap().as_array().unwrap().len(), 2);
    }

    #[test]
    fn tap_frame_meter_only_round_trip() {
        let f = TapFrame::new(vec![TapSlotFrame {
            s: 0,
            p: 0.5,
            r: 0.25,
            ..Default::default()
        }]);
        let json = serde_json::to_string(&f).unwrap();
        // Optional fields elided.
        assert!(!json.contains("\"w\""));
        assert!(!json.contains("\"m\""));
        assert!(!json.contains("\"g\""));
        assert!(!json.contains("\"t\""));
        let back: TapFrame = serde_json::from_str(&json).unwrap();
        assert_eq!(back, f);
        assert_eq!(back.v, TapFrame::VERSION);
    }

    #[test]
    fn tap_frame_scope_round_trip() {
        let f = TapFrame::new(vec![TapSlotFrame {
            s: 2,
            p: 0.9,
            r: 0.6,
            w: Some(vec![0.0, 0.1, -0.1, 0.0]),
            ..Default::default()
        }]);
        let json = serde_json::to_string(&f).unwrap();
        assert!(json.contains("\"w\""));
        let back: TapFrame = serde_json::from_str(&json).unwrap();
        assert_eq!(back, f);
    }

    #[test]
    fn tap_frame_spectrum_round_trip() {
        let f = TapFrame::new(vec![TapSlotFrame {
            s: 1,
            p: 0.0,
            r: 0.0,
            m: Some(vec![0.0; 8]),
            ..Default::default()
        }]);
        let json = serde_json::to_string(&f).unwrap();
        let back: TapFrame = serde_json::from_str(&json).unwrap();
        assert_eq!(back, f);
    }

    #[test]
    fn tap_frame_compound_round_trip() {
        let f = TapFrame::new(vec![
            TapSlotFrame { s: 0, p: 0.4, r: 0.2, ..Default::default() },
            TapSlotFrame {
                s: 1,
                p: 0.7,
                r: 0.5,
                w: Some(vec![0.1, 0.2]),
                m: Some(vec![0.3, 0.4, 0.5]),
                ..Default::default()
            },
        ]);
        let json = serde_json::to_string(&f).unwrap();
        let back: TapFrame = serde_json::from_str(&json).unwrap();
        assert_eq!(back, f);
        assert_eq!(back.slots.len(), 2);
    }

    #[test]
    fn tap_summary_json_round_trip() {
        let original = TapSummary {
            name: "lead".into(),
            slot: 3,
            kind: "osc".into(),
            components: vec!["osc".into()],
        };
        let json = serde_json::to_string(&original).unwrap();
        let parsed: TapSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, original);
    }
}
