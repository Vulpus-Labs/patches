//! Webview-facing types: `GuiSnapshot` projection, `Intent` wire
//! format, `TapFrame` per-tick samples. `Controller` owns the model;
//! `Controller::snapshot` builds [`GuiSnapshot`].

use patches_core::source_map::SourceMap;
use patches_diagnostics::{source_line_col, RenderedDiagnostic, Severity};
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
    /// Scope auto-trigger ("snap") toggle. Pure UI state, but persisted
    /// alongside the numeric opts so it survives window close/reopen
    /// and host save/load.
    pub scope_snap: bool,
    /// Spectrum display mode: `false` = curve, `true` = heatmap.
    pub spectrum_heatmap: bool,
}

impl Default for TapDisplayOpts {
    fn default() -> Self {
        Self {
            spectrum_fft_size: 1024,
            scope_decimation: 16,
            scope_window_samples: 512,
            scope_snap: false,
            spectrum_heatmap: false,
        }
    }
}

/// Versioned snapshot of controller state projected to a shape a
/// webview can consume. Built by [`Controller::snapshot`].
///
/// Keep the field set small and string-typed — the JS side is hand-written.
/// Bump `v` whenever the shape changes in a breaking way.
#[derive(Serialize, PartialEq, Eq)]
pub struct GuiSnapshot {
    pub v: u32,
    pub file_path: Option<String>,
    pub module_paths: Vec<String>,
    pub module_names: Vec<String>,
    pub status_log: Vec<String>,
    pub halt_message: Option<String>,
    pub diagnostics: Vec<DiagnosticSummary>,
    pub taps: Vec<TapSummary>,
    /// Per-slot display options projected for the webview to restore
    /// selector values after a window close/reopen or state reload.
    /// Keyed by slot index. Ticket 0752 follow-up.
    pub tap_opts: std::collections::HashMap<String, TapDisplayOpts>,
    /// Preset names available for the current patch (ticket 0777).
    pub preset_names: Vec<String>,
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
    pub const VERSION: u32 = 7;
}

pub(crate) fn summarise_diagnostics_pub(view: &DiagnosticView) -> Vec<DiagnosticSummary> {
    summarise_diagnostics(view)
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
/// Lowered to [`Action`](crate::controller::Action) via
/// [`Intent::into_action`]. Ticket 0671.
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
        name: String,
        spectrum_fft_size: Option<usize>,
        scope_decimation: Option<usize>,
        scope_window_samples: Option<usize>,
        scope_snap: Option<bool>,
        spectrum_heatmap: Option<bool>,
    },
    /// JS bundle finished loading and `window.__patches` is wired up.
    /// Host-side handler clears push-dedupe caches so the next snapshot
    /// / tap-frame goes through unconditionally. Ticket 0752.
    Ready,
}

impl Intent {
    /// Lower a webview-posted intent into an [`Action`] for the
    /// controller queue. Returns `None` for `Ready`, which the shell
    /// handles inline (clears push-dedupe caches; not a state action).
    pub fn into_action(self) -> Option<crate::controller::Action> {
        use crate::controller::Action;
        Some(match self {
            Intent::Browse => Action::Browse,
            Intent::Reload => Action::Reload,
            Intent::Rescan => Action::Rescan,
            Intent::AddPath => Action::AddModulePath,
            Intent::RemovePath { index } => Action::RemoveModulePath(index),
            Intent::SetTapOpts {
                name,
                spectrum_fft_size,
                scope_decimation,
                scope_window_samples,
                scope_snap,
                spectrum_heatmap,
            } => Action::SetTapOpts {
                name,
                spectrum_fft_size,
                scope_decimation,
                scope_window_samples,
                scope_snap,
                spectrum_heatmap,
            },
            Intent::Ready => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Intent {
        serde_json::from_str(json).expect("intent json")
    }

    #[test]
    fn intent_lowers_to_action() {
        use crate::controller::Action;
        assert!(matches!(parse(r#"{"kind":"reload"}"#).into_action(), Some(Action::Reload)));
        assert!(matches!(parse(r#"{"kind":"browse"}"#).into_action(), Some(Action::Browse)));
        assert!(matches!(parse(r#"{"kind":"rescan"}"#).into_action(), Some(Action::Rescan)));
        assert!(matches!(parse(r#"{"kind":"add_path"}"#).into_action(), Some(Action::AddModulePath)));
        match parse(r#"{"kind":"remove_path","index":2}"#).into_action() {
            Some(Action::RemoveModulePath(2)) => {}
            other => panic!("got {other:?}"),
        }
        assert!(parse(r#"{"kind":"ready"}"#).into_action().is_none());
    }

    #[test]
    fn snapshot_carries_taps_in_slot_order() {
        let mut c = crate::Controller::new();
        c.taps = vec![
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
        ];
        let snap = c.snapshot();
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
