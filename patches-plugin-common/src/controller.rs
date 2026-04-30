//! Plugin controller — single mutation entry point for persistable
//! plugin state. ADR 0061.
//!
//! Pure scaffolding: the types compile and `Controller::apply` returns
//! `StateDelta::default()` for every variant. Subsequent tickets (0758–
//! 0763) migrate handler logic out of `patches-clap` into this module.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;

use patches_diagnostics::RenderedDiagnostic;
use patches_engine::HaltInfoSnapshot;
use patches_registry::Registry;

use crate::gui::{
    DiagnosticView, GuiSnapshot, TapDisplayOpts, TapSummary, STATUS_LOG_CAPACITY,
};

/// Snapshot of persistable state read/written by the host's
/// `state_load` / `state_save` hooks. Mirrors the wire format used in
/// `patches-clap`'s extensions.rs today; later tickets may swap to a
/// versioned struct without touching call sites.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SerializedState {
    pub file_path: Option<PathBuf>,
    pub dsl_source: String,
    pub module_paths: Vec<PathBuf>,
    pub tap_opts: HashMap<usize, TapDisplayOpts>,
    pub window_size: Option<(u32, u32)>,
}

/// Outcome of a cheap rescan probe — diff against the currently
/// registered builders without keeping any libraries loaded.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RescanProbe {
    pub added: Vec<String>,
    pub replaced: Vec<String>,
    pub removed: Vec<String>,
    pub unchanged: Vec<String>,
    pub errors: Vec<String>,
}

impl RescanProbe {
    pub fn is_actionable(&self) -> bool {
        !(self.added.is_empty() && self.replaced.is_empty() && self.removed.is_empty())
    }
}

/// Closed set of state transitions. Webview JSON intents and host
/// events both deserialise / lower into this enum.
#[derive(Debug, Clone)]
pub enum Action {
    // UI gestures
    Browse,
    Reload,
    LoadPath(PathBuf),
    AddModulePath,
    AddModulePathDirect(PathBuf),
    RemoveModulePath(usize),
    Rescan,
    SetTapOpts {
        slot: usize,
        spectrum_fft_size: Option<usize>,
        scope_decimation: Option<usize>,
        scope_window_samples: Option<usize>,
        scope_snap: Option<bool>,
        spectrum_heatmap: Option<bool>,
    },
    SetWindowSize(u32, u32),

    // Host events
    Activate,
    Deactivate,
    StateLoad(SerializedState),
    HaltObserved(HaltInfoSnapshot),
    PlanAdopted,
    DiagnosticsDrained(Vec<RenderedDiagnostic>),
}

/// What the shell must do after a `Controller::apply` call.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StateDelta {
    /// Call `mark_state_dirty` on the host.
    pub persistable_changed: bool,
    /// Call `request_restart` on the host.
    pub requires_restart: bool,
    /// Republish a fresh `GuiSnapshot` to the webview.
    pub snapshot_changed: bool,
    /// Run `Env::compile_and_push_plan` for the current source.
    pub plan_recompile: bool,
}

/// Side-effect surface the controller calls into. Host shells supply
/// the concrete impl; tests supply fakes.
pub trait Env {
    fn read_file(&mut self, path: &std::path::Path) -> std::io::Result<String>;
    fn pick_file(&mut self) -> Option<PathBuf>;
    fn pick_folder(&mut self) -> Option<PathBuf>;
    /// Compile DSL source into an `ExecutionPlan` and push it to the
    /// audio thread. Returns a human-readable error on failure.
    fn compile_and_push_plan(&mut self, source: &str) -> Result<(), String>;
    /// Full scan: load module bundles from `paths` and merge their
    /// builders into the controller's registry.
    fn scan_paths(&mut self, paths: &[PathBuf], registry: &mut Registry);
    /// Cheap probe: read manifests only and diff against `registry`.
    fn probe_paths(&mut self, paths: &[PathBuf], registry: &Registry) -> RescanProbe;
}

/// Persistable + derived plugin model. Single owner of the state that
/// `Action`s mutate.
#[derive(Default)]
pub struct Controller {
    // Persistable model.
    pub file_path: Option<PathBuf>,
    pub dsl_source: String,
    pub module_paths: Vec<PathBuf>,
    pub tap_opts: HashMap<usize, TapDisplayOpts>,
    pub window_size: Option<(u32, u32)>,

    // Derived / live state.
    pub registry: Registry,
    pub status_log: VecDeque<String>,
    pub diagnostic_view: DiagnosticView,
    pub halt: Option<HaltInfoSnapshot>,
    pub taps: Vec<TapSummary>,
    pub module_names: Vec<String>,
}

impl Controller {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a status line, evicting the oldest entries past
    /// [`STATUS_LOG_CAPACITY`].
    pub fn push_status(&mut self, msg: impl Into<String>) {
        if self.status_log.len() >= STATUS_LOG_CAPACITY {
            self.status_log.pop_front();
        }
        self.status_log.push_back(msg.into());
    }

    /// Apply an action. Stub: returns `StateDelta::default()` for every
    /// variant. Handlers move in subsequent tickets.
    pub fn apply(&mut self, action: Action, _env: &mut dyn Env) -> StateDelta {
        let _ = action;
        StateDelta::default()
    }

    /// Project current state into the webview-facing snapshot shape.
    pub fn snapshot(&self) -> GuiSnapshot {
        GuiSnapshot {
            v: GuiSnapshot::VERSION,
            file_path: self.file_path.as_ref().map(|p| p.display().to_string()),
            module_paths: self
                .module_paths
                .iter()
                .map(|p| p.display().to_string())
                .collect(),
            module_names: self.module_names.clone(),
            status_log: self.status_log.iter().cloned().collect(),
            // Request-flag fields stay in the snapshot for now so the
            // webview shape doesn't shift. They're always false: the
            // controller drains actions instead of latching flags.
            browse_requested: false,
            reload_requested: false,
            add_path_requested: false,
            rescan_requested: false,
            remove_path_index: None,
            halt_message: self.halt.as_ref().map(format_halt),
            diagnostics: crate::gui::summarise_diagnostics_pub(&self.diagnostic_view),
            taps: self.taps.clone(),
            tap_opts: self.tap_opts.clone(),
        }
    }
}

fn format_halt(h: &HaltInfoSnapshot) -> String {
    let first = h.payload.lines().next().unwrap_or("");
    format!(
        "Engine halted: module {:?} (slot {}) panicked: {}",
        h.module_name, h.slot, first,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NullEnv;
    impl Env for NullEnv {
        fn read_file(&mut self, _path: &std::path::Path) -> std::io::Result<String> {
            Ok(String::new())
        }
        fn pick_file(&mut self) -> Option<PathBuf> { None }
        fn pick_folder(&mut self) -> Option<PathBuf> { None }
        fn compile_and_push_plan(&mut self, _source: &str) -> Result<(), String> { Ok(()) }
        fn scan_paths(&mut self, _paths: &[PathBuf], _registry: &mut Registry) {}
        fn probe_paths(&mut self, _paths: &[PathBuf], _registry: &Registry) -> RescanProbe {
            RescanProbe::default()
        }
    }

    fn all_actions() -> Vec<Action> {
        vec![
            Action::Browse,
            Action::Reload,
            Action::LoadPath("/tmp/x.patches".into()),
            Action::AddModulePath,
            Action::AddModulePathDirect("/tmp/mods".into()),
            Action::RemoveModulePath(0),
            Action::Rescan,
            Action::SetTapOpts {
                slot: 0,
                spectrum_fft_size: Some(1024),
                scope_decimation: None,
                scope_window_samples: None,
                scope_snap: None,
                spectrum_heatmap: None,
            },
            Action::SetWindowSize(800, 600),
            Action::Activate,
            Action::Deactivate,
            Action::StateLoad(SerializedState::default()),
            Action::PlanAdopted,
            Action::DiagnosticsDrained(Vec::new()),
        ]
    }

    #[test]
    fn apply_returns_default_delta_for_every_action() {
        let mut c = Controller::new();
        let mut env = NullEnv;
        for a in all_actions() {
            let d = c.apply(a, &mut env);
            assert_eq!(d, StateDelta::default());
        }
    }

    #[test]
    fn snapshot_round_trip_matches_gui_state_projection() {
        let mut c = Controller::new();
        c.file_path = Some("/tmp/a.patches".into());
        c.module_paths.push("/tmp/mods".into());
        c.module_names.push("osc".to_string());
        c.push_status("hello");
        c.taps.push(TapSummary {
            name: "kick".into(),
            slot: 0,
            kind: "meter".into(),
            components: vec!["meter".into()],
        });
        c.tap_opts.insert(0, TapDisplayOpts::default());

        let snap = c.snapshot();
        assert_eq!(snap.v, GuiSnapshot::VERSION);
        assert_eq!(snap.file_path.as_deref(), Some("/tmp/a.patches"));
        assert_eq!(snap.module_paths, vec!["/tmp/mods".to_string()]);
        assert_eq!(snap.module_names, vec!["osc".to_string()]);
        assert_eq!(snap.status_log, vec!["hello".to_string()]);
        assert_eq!(snap.taps.len(), 1);
        assert!(snap.tap_opts.contains_key(&0));
        assert!(!snap.browse_requested);
        assert!(!snap.reload_requested);

        let json = serde_json::to_string(&snap).unwrap();
        assert!(json.contains("\"v\":5"));
    }

    #[test]
    fn rescan_probe_actionable_iff_diff_nonempty() {
        let p = RescanProbe::default();
        assert!(!p.is_actionable());
        let p = RescanProbe {
            added: vec!["x".into()],
            ..Default::default()
        };
        assert!(p.is_actionable());
        let p = RescanProbe {
            unchanged: vec!["y".into()],
            ..Default::default()
        };
        assert!(!p.is_actionable());
    }
}
