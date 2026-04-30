//! Plugin controller — single mutation entry point for persistable
//! plugin state. ADR 0061.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use patches_diagnostics::RenderedDiagnostic;
use patches_engine::HaltInfoSnapshot;
use patches_registry::Registry;

use crate::gui::{
    DiagnosticView, GuiSnapshot, TapDisplayOpts, TapSummary, STATUS_LOG_CAPACITY,
};

/// Snapshot of persistable state read/written by the host's
/// `state_load` / `state_save` hooks.
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

/// Result of a successful compile + plan push.
#[derive(Clone, Debug, Default)]
pub struct CompileSuccess {
    pub taps: Vec<TapSummary>,
    pub warnings: Vec<RenderedDiagnostic>,
}

/// Result of a failed compile.
#[derive(Clone)]
pub struct CompileFailure {
    pub message: String,
    pub view: DiagnosticView,
}

/// Summary of a fresh-registry scan, returned by `Env::preview_scan`.
#[derive(Clone, Debug, Default)]
pub struct ScanDetails {
    pub summary: String,
    pub details: Vec<String>,
    pub module_names: Vec<String>,
}

/// Closed set of state transitions. Webview JSON intents and host
/// events both lower into this enum.
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
    pub persistable_changed: bool,
    pub requires_restart: bool,
    pub snapshot_changed: bool,
    pub plan_recompile: bool,
}

/// Side-effect surface the controller calls into.
pub trait Env {
    fn pick_file(&mut self) -> Option<PathBuf>;
    fn pick_folder(&mut self) -> Option<PathBuf>;
    fn read_file(&mut self, path: &Path) -> std::io::Result<String>;
    /// Compile DSL source and push the plan to the audio thread.
    fn compile_and_push_plan(
        &mut self,
        source: &str,
        file_path: Option<&Path>,
        registry: &Registry,
    ) -> Result<CompileSuccess, CompileFailure>;
    /// Scan `paths` into a fresh default registry and return a summary.
    /// Used for the rescan preview before host restart.
    fn preview_scan(&mut self, paths: &[PathBuf]) -> ScanDetails;
    /// Build a fresh default registry, scan `paths` into it, and return
    /// both. Used by `Action::Activate` to rebuild the live registry.
    fn reset_and_scan(&mut self, paths: &[PathBuf]) -> (Registry, ScanDetails);
}

/// Persistable + derived plugin model.
#[derive(Default)]
pub struct Controller {
    // Persistable.
    pub file_path: Option<PathBuf>,
    pub dsl_source: String,
    pub module_paths: Vec<PathBuf>,
    pub tap_opts: HashMap<usize, TapDisplayOpts>,
    pub window_size: Option<(u32, u32)>,

    // Derived / live.
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

    pub fn push_status(&mut self, msg: impl Into<String>) {
        if self.status_log.len() >= STATUS_LOG_CAPACITY {
            self.status_log.pop_front();
        }
        self.status_log.push_back(msg.into());
    }

    /// Apply an action.
    pub fn apply(&mut self, action: Action, env: &mut dyn Env) -> StateDelta {
        match action {
            Action::Browse => {
                if let Some(path) = env.pick_file() {
                    return self.load_path(path, "Loaded", env);
                }
                StateDelta::default()
            }
            Action::Reload => {
                if let Some(path) = self.file_path.clone() {
                    return self.load_path(path, "Reloaded", env);
                }
                StateDelta::default()
            }
            Action::LoadPath(path) => self.load_path(path, "Loaded", env),
            Action::AddModulePath => {
                if let Some(dir) = env.pick_folder() {
                    return self.add_module_path(dir);
                }
                StateDelta::default()
            }
            Action::AddModulePathDirect(dir) => self.add_module_path(dir),
            Action::RemoveModulePath(idx) => {
                if idx >= self.module_paths.len() {
                    return StateDelta::default();
                }
                let removed = self.module_paths.remove(idx);
                self.push_status(format!(
                    "Removed module path: {} (press Rescan to apply)",
                    removed.display()
                ));
                StateDelta {
                    persistable_changed: true,
                    snapshot_changed: true,
                    ..Default::default()
                }
            }
            Action::Rescan => {
                let scan = env.preview_scan(&self.module_paths);
                self.module_names = scan.module_names;
                self.push_status(format!("Rescan: {}", scan.summary));
                for line in &scan.details {
                    self.push_status(line.clone());
                }
                self.diagnostic_view = DiagnosticView::default();
                StateDelta {
                    requires_restart: true,
                    snapshot_changed: true,
                    ..Default::default()
                }
            }
            Action::SetTapOpts {
                slot,
                spectrum_fft_size,
                scope_decimation,
                scope_window_samples,
                scope_snap,
                spectrum_heatmap,
            } => {
                let entry = self.tap_opts.entry(slot).or_default();
                let before = *entry;
                if let Some(n) = spectrum_fft_size {
                    entry.spectrum_fft_size = n;
                }
                if let Some(d) = scope_decimation {
                    entry.scope_decimation = d;
                }
                if let Some(w) = scope_window_samples {
                    entry.scope_window_samples = w;
                }
                if let Some(b) = scope_snap {
                    entry.scope_snap = b;
                }
                if let Some(b) = spectrum_heatmap {
                    entry.spectrum_heatmap = b;
                }
                let changed = *entry != before;
                StateDelta {
                    persistable_changed: changed,
                    snapshot_changed: changed,
                    ..Default::default()
                }
            }
            Action::SetWindowSize(w, h) => {
                let next = Some((w, h));
                if self.window_size == next {
                    return StateDelta::default();
                }
                self.window_size = next;
                StateDelta {
                    persistable_changed: true,
                    ..Default::default()
                }
            }
            Action::Activate => {
                let (registry, scan) = env.reset_and_scan(&self.module_paths);
                self.registry = registry;
                self.module_names = scan.module_names;
                if !self.module_paths.is_empty() {
                    self.push_status(format!("Module scan: {}", scan.summary));
                    for line in scan.details {
                        self.push_status(line);
                    }
                }
                if !self.dsl_source.is_empty() {
                    match env.compile_and_push_plan(
                        &self.dsl_source,
                        self.file_path.as_deref(),
                        &self.registry,
                    ) {
                        Ok(success) => {
                            self.taps = success.taps;
                            self.diagnostic_view = DiagnosticView::default();
                            if !success.warnings.is_empty() {
                                self.diagnostic_view.diagnostics.extend(success.warnings);
                            }
                        }
                        Err(failure) => {
                            self.diagnostic_view = failure.view;
                            self.push_status(format!("Error: {}", failure.message));
                        }
                    }
                }
                StateDelta {
                    snapshot_changed: true,
                    ..Default::default()
                }
            }
            Action::StateLoad(s) => {
                self.file_path = s.file_path;
                self.dsl_source = s.dsl_source;
                self.module_paths = s.module_paths;
                self.tap_opts = s.tap_opts;
                self.window_size = s.window_size;
                StateDelta {
                    snapshot_changed: true,
                    ..Default::default()
                }
            }
            // Remaining host events handled in later tickets.
            _ => StateDelta::default(),
        }
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
            halt_message: self.halt.as_ref().map(format_halt),
            diagnostics: crate::gui::summarise_diagnostics_pub(&self.diagnostic_view),
            taps: self.taps.clone(),
            tap_opts: self.tap_opts.clone(),
        }
    }

    fn load_path(&mut self, path: PathBuf, success_msg: &str, env: &mut dyn Env) -> StateDelta {
        let mut delta = StateDelta {
            snapshot_changed: true,
            ..Default::default()
        };
        let path_changed = self.file_path.as_ref() != Some(&path);
        if path_changed {
            self.file_path = Some(path.clone());
            delta.persistable_changed = true;
        }
        let source = match env.read_file(&path) {
            Ok(s) => s,
            Err(e) => {
                self.push_status(format!("Read error: {e}"));
                return delta;
            }
        };
        let source_changed = self.dsl_source != source;
        self.dsl_source = source;
        if source_changed {
            delta.persistable_changed = true;
        }
        match env.compile_and_push_plan(&self.dsl_source, self.file_path.as_deref(), &self.registry)
        {
            Ok(success) => {
                self.taps = success.taps;
                self.diagnostic_view = DiagnosticView::default();
                if !success.warnings.is_empty() {
                    self.diagnostic_view.diagnostics.extend(success.warnings);
                }
                self.push_status(success_msg);
            }
            Err(failure) => {
                self.diagnostic_view = failure.view;
                self.push_status(format!("Error: {}", failure.message));
            }
        }
        delta
    }

    fn add_module_path(&mut self, dir: PathBuf) -> StateDelta {
        if self.module_paths.iter().any(|p| p == &dir) {
            return StateDelta::default();
        }
        self.push_status(format!(
            "Added module path: {} (press Rescan to load)",
            dir.display()
        ));
        self.module_paths.push(dir);
        StateDelta {
            persistable_changed: true,
            snapshot_changed: true,
            ..Default::default()
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

    #[derive(Default)]
    struct FakeEnv {
        files: HashMap<PathBuf, String>,
        next_file: Option<PathBuf>,
        next_folder: Option<PathBuf>,
        compile_ok: bool,
        compile_taps: Vec<TapSummary>,
        scan: ScanDetails,
        compiled_sources: Vec<String>,
    }

    impl Env for FakeEnv {
        fn pick_file(&mut self) -> Option<PathBuf> {
            self.next_file.take()
        }
        fn pick_folder(&mut self) -> Option<PathBuf> {
            self.next_folder.take()
        }
        fn read_file(&mut self, path: &Path) -> std::io::Result<String> {
            self.files
                .get(path)
                .cloned()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "missing"))
        }
        fn compile_and_push_plan(
            &mut self,
            source: &str,
            _file_path: Option<&Path>,
            _registry: &Registry,
        ) -> Result<CompileSuccess, CompileFailure> {
            self.compiled_sources.push(source.to_string());
            if self.compile_ok {
                Ok(CompileSuccess {
                    taps: self.compile_taps.clone(),
                    warnings: Vec::new(),
                })
            } else {
                Err(CompileFailure {
                    message: "boom".into(),
                    view: DiagnosticView::default(),
                })
            }
        }
        fn preview_scan(&mut self, _paths: &[PathBuf]) -> ScanDetails {
            self.scan.clone()
        }
        fn reset_and_scan(&mut self, _paths: &[PathBuf]) -> (Registry, ScanDetails) {
            (Registry::default(), self.scan.clone())
        }
    }

    fn ok_env() -> FakeEnv {
        FakeEnv {
            compile_ok: true,
            ..Default::default()
        }
    }

    #[test]
    fn browse_with_pick_loads_compiles_and_dirties() {
        let path = PathBuf::from("/tmp/a.patches");
        let mut env = ok_env();
        env.next_file = Some(path.clone());
        env.files.insert(path.clone(), "x".into());
        let mut c = Controller::new();
        let d = c.apply(Action::Browse, &mut env);
        assert!(d.persistable_changed);
        assert!(d.snapshot_changed);
        assert!(!d.requires_restart);
        assert_eq!(c.file_path, Some(path));
        assert_eq!(c.dsl_source, "x");
        assert_eq!(env.compiled_sources, vec!["x".to_string()]);
    }

    #[test]
    fn browse_cancelled_is_no_op() {
        let mut env = ok_env();
        let mut c = Controller::new();
        let d = c.apply(Action::Browse, &mut env);
        assert_eq!(d, StateDelta::default());
    }

    #[test]
    fn reload_without_path_is_no_op() {
        let mut env = ok_env();
        let mut c = Controller::new();
        let d = c.apply(Action::Reload, &mut env);
        assert_eq!(d, StateDelta::default());
    }

    #[test]
    fn reload_re_reads_current_path() {
        let path = PathBuf::from("/tmp/a.patches");
        let mut env = ok_env();
        env.files.insert(path.clone(), "src".into());
        let mut c = Controller::new();
        c.file_path = Some(path);
        let d = c.apply(Action::Reload, &mut env);
        assert!(d.snapshot_changed);
        assert!(d.persistable_changed); // dsl_source changed from empty
        assert_eq!(c.dsl_source, "src");
    }

    #[test]
    fn load_path_records_failure_diagnostic() {
        let path = PathBuf::from("/tmp/x.patches");
        let mut env = FakeEnv {
            compile_ok: false,
            ..Default::default()
        };
        env.files.insert(path.clone(), "bad".into());
        let mut c = Controller::new();
        let _d = c.apply(Action::LoadPath(path), &mut env);
        let last = c.status_log.back().cloned().unwrap_or_default();
        assert!(last.starts_with("Error:"));
    }

    #[test]
    fn add_module_path_dedupes() {
        let mut env = ok_env();
        let mut c = Controller::new();
        let dir = PathBuf::from("/tmp/m");
        let d1 = c.apply(Action::AddModulePathDirect(dir.clone()), &mut env);
        assert!(d1.persistable_changed);
        let d2 = c.apply(Action::AddModulePathDirect(dir), &mut env);
        assert_eq!(d2, StateDelta::default());
        assert_eq!(c.module_paths.len(), 1);
    }

    #[test]
    fn add_module_path_with_picker() {
        let mut env = ok_env();
        env.next_folder = Some("/tmp/m".into());
        let mut c = Controller::new();
        let d = c.apply(Action::AddModulePath, &mut env);
        assert!(d.persistable_changed);
        assert_eq!(c.module_paths, vec![PathBuf::from("/tmp/m")]);
    }

    #[test]
    fn remove_module_path_in_range() {
        let mut env = ok_env();
        let mut c = Controller::new();
        c.module_paths = vec!["/tmp/a".into(), "/tmp/b".into()];
        let d = c.apply(Action::RemoveModulePath(0), &mut env);
        assert!(d.persistable_changed);
        assert_eq!(c.module_paths, vec![PathBuf::from("/tmp/b")]);
    }

    #[test]
    fn remove_module_path_out_of_range_is_no_op() {
        let mut env = ok_env();
        let mut c = Controller::new();
        let d = c.apply(Action::RemoveModulePath(7), &mut env);
        assert_eq!(d, StateDelta::default());
    }

    #[test]
    fn rescan_requires_restart_and_clears_diagnostics() {
        let mut env = ok_env();
        env.scan = ScanDetails {
            summary: "1 loaded, 0 replaced, 0 skipped, 0 errors".into(),
            details: vec![],
            module_names: vec!["Gain".into()],
        };
        let mut c = Controller::new();
        let d = c.apply(Action::Rescan, &mut env);
        assert!(d.requires_restart);
        assert!(d.snapshot_changed);
        assert!(!d.persistable_changed);
        assert_eq!(c.module_names, vec!["Gain".to_string()]);
        assert!(c.diagnostic_view.diagnostics.is_empty());
    }

    #[test]
    fn set_window_size_dirties_only_when_changed() {
        let mut env = ok_env();
        let mut c = Controller::new();
        let d1 = c.apply(Action::SetWindowSize(800, 600), &mut env);
        assert!(d1.persistable_changed);
        assert_eq!(c.window_size, Some((800, 600)));
        let d2 = c.apply(Action::SetWindowSize(800, 600), &mut env);
        assert_eq!(d2, StateDelta::default());
    }

    #[test]
    fn set_tap_opts_marks_persistable_when_changed() {
        let mut env = ok_env();
        let mut c = Controller::new();
        let d = c.apply(
            Action::SetTapOpts {
                slot: 0,
                spectrum_fft_size: Some(2048),
                scope_decimation: None,
                scope_window_samples: None,
                scope_snap: None,
                spectrum_heatmap: None,
            },
            &mut env,
        );
        assert!(d.persistable_changed);
        assert_eq!(c.tap_opts.get(&0).unwrap().spectrum_fft_size, 2048);

        // Re-apply same value — no change.
        let d2 = c.apply(
            Action::SetTapOpts {
                slot: 0,
                spectrum_fft_size: Some(2048),
                scope_decimation: None,
                scope_window_samples: None,
                scope_snap: None,
                spectrum_heatmap: None,
            },
            &mut env,
        );
        assert!(!d2.persistable_changed);
    }

    #[test]
    fn unimplemented_host_events_return_default_delta() {
        let mut env = ok_env();
        let mut c = Controller::new();
        for a in [
            Action::Deactivate,
            Action::PlanAdopted,
            Action::DiagnosticsDrained(Vec::new()),
        ] {
            assert_eq!(c.apply(a, &mut env), StateDelta::default());
        }
    }

    #[test]
    fn activate_rebuilds_registry_and_compiles() {
        let mut env = ok_env();
        env.scan = ScanDetails {
            summary: "1 loaded".into(),
            details: vec![],
            module_names: vec!["Gain".into()],
        };
        let mut c = Controller::new();
        c.module_paths.push("/tmp/m".into());
        c.dsl_source = "src".into();
        let d = c.apply(Action::Activate, &mut env);
        assert!(d.snapshot_changed);
        assert_eq!(c.module_names, vec!["Gain".to_string()]);
        assert_eq!(env.compiled_sources, vec!["src".to_string()]);
        assert!(c.status_log.iter().any(|s| s.starts_with("Module scan:")));
    }

    #[test]
    fn activate_with_empty_paths_skips_scan_status() {
        let mut env = ok_env();
        let mut c = Controller::new();
        c.apply(Action::Activate, &mut env);
        assert!(!c.status_log.iter().any(|s| s.starts_with("Module scan:")));
    }

    #[test]
    fn state_load_replaces_persistable_fields() {
        let mut env = ok_env();
        let mut c = Controller::new();
        c.dsl_source = "stale".into();
        let s = SerializedState {
            file_path: Some("/tmp/x.patches".into()),
            dsl_source: "fresh".into(),
            module_paths: vec!["/tmp/m".into()],
            tap_opts: Default::default(),
            window_size: Some((800, 600)),
        };
        let d = c.apply(Action::StateLoad(s), &mut env);
        assert!(d.snapshot_changed);
        assert!(!d.persistable_changed);
        assert_eq!(c.dsl_source, "fresh");
        assert_eq!(c.file_path.as_deref(), Some(std::path::Path::new("/tmp/x.patches")));
        assert_eq!(c.module_paths, vec![PathBuf::from("/tmp/m")]);
        assert_eq!(c.window_size, Some((800, 600)));
    }

    #[test]
    fn snapshot_carries_status_and_paths() {
        let mut c = Controller::new();
        c.file_path = Some("/tmp/a.patches".into());
        c.module_paths.push("/tmp/m".into());
        c.push_status("hello");
        let snap = c.snapshot();
        assert_eq!(snap.v, GuiSnapshot::VERSION);
        assert_eq!(snap.file_path.as_deref(), Some("/tmp/a.patches"));
        assert_eq!(snap.module_paths, vec!["/tmp/m".to_string()]);
        assert_eq!(snap.status_log, vec!["hello".to_string()]);
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
    }
}
