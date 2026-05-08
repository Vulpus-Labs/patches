//! Plugin controller — single mutation entry point for persistable
//! plugin state. ADR 0061.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use patches_diagnostics::RenderedDiagnostic;
use patches_dsl::host_control_manifest::HostControlManifest;
use patches_engine::HaltInfoSnapshot;
use patches_registry::Registry;
use serde::{Deserialize, Serialize};

use crate::gui::{
    DiagnosticView, GuiSnapshot, ScopeMode, SpectrumRender, TapDisplayOpts, TapSummary,
    STATUS_LOG_CAPACITY,
};

/// Patch identity — *which* patch is loaded. Local-machine-only:
/// path is meaningful only on the originating machine, source is the
/// authoritative content. Persisted in the CLAP state envelope so a
/// project file can re-find its patch on reopen. Excluded from
/// presets (preset semantics are "settings to apply to *some* patch").
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatchIdentity {
    pub file_path: Option<PathBuf>,
    pub dsl_source: String,
}

/// Portable user settings that move with presets. Cross-patch-safe:
/// every map keys by name, so applying to a different patch
/// silently drops bindings that don't resolve.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PersistedSettings {
    /// Host-control values keyed by name. Populated by ADR 0057;
    /// empty until then.
    pub host_controls: HashMap<String, f32>,
    pub tap_opts: HashMap<String, TapDisplayOpts>,
    pub window_size: Option<(u32, u32)>,
    pub module_paths: Vec<PathBuf>,
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
    /// Host-control manifest paired with this compile (ADR 0057,
    /// ticket 0811). Empty when the patch declares no host controls.
    pub host_control_manifest: Arc<HostControlManifest>,
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

/// One per-tap display option. Externally tagged when deserialised so
/// the wire form is `{spectrum_fft_size: 1024}` etc.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TapOpt {
    SpectrumFftSize(usize),
    ScopeDecimation(usize),
    ScopeWindowSamples(usize),
    ScopeSnap(bool),
    SpectrumHeatmap(bool),
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
    SetTapOpt {
        name: String,
        opt: TapOpt,
    },
    SetWindowSize(u32, u32),

    // Host events
    Activate,
    Deactivate,
    StateLoad {
        identity: PatchIdentity,
        settings: PersistedSettings,
    },
    /// Save current persistable settings as a named preset under the
    /// current patch identity (ADR 0063 §6; ticket 0777).
    SavePreset {
        name: String,
    },
    /// Load a named preset and apply its [`PersistedSettings`] to the
    /// current patch. Names that don't resolve in the current patch
    /// degrade gracefully — they sit in the controller until
    /// reconciliation against a fresh manifest drops them.
    LoadPreset {
        name: String,
    },
    HaltObserved(Option<HaltInfoSnapshot>),
    DiagnosticsDrained(Vec<String>),
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
    /// Cheap probe: read each candidate bundle's manifest and diff
    /// against `registry` without keeping libraries permanently
    /// loaded. ABI / dlopen errors land in `RescanProbe::errors`.
    fn probe_paths(&mut self, paths: &[PathBuf], registry: &Registry) -> RescanProbe;
    /// Full scan into the supplied registry (modules merged in place).
    /// Used by `Action::Reload` / `Action::LoadPath` to ensure newly-
    /// added module paths are honoured before compile.
    fn scan_into(&mut self, paths: &[PathBuf], registry: &mut Registry) -> ScanDetails;
    /// Build a fresh default registry, scan `paths` into it, and return
    /// both. Used by `Action::Activate` to rebuild the live registry.
    fn reset_and_scan(&mut self, paths: &[PathBuf]) -> (Registry, ScanDetails);

    /// Resolve the sidecar location for a given patch path. `None`
    /// means this env doesn't use sidecars (CLAP — host owns persistence).
    /// ADR 0063 §5; ticket 0775.
    fn sidecar_path(&self, _patch_path: &Path) -> Option<PathBuf> {
        None
    }
    /// Read the sidecar at `path`. `Ok(None)` for missing-but-not-error
    /// (the common case on a fresh patch). Default impl: not supported.
    fn load_sidecar(&mut self, _path: &Path) -> std::io::Result<Option<PersistedSettings>> {
        Ok(None)
    }
    /// Write `settings` to `path`. Default impl: no-op (CLAP).
    fn save_sidecar(
        &mut self,
        _path: &Path,
        _settings: &PersistedSettings,
    ) -> std::io::Result<()> {
        Ok(())
    }

    /// Resolve the on-disk preset path for `(patch_stem, preset_name)`.
    /// `None` means presets are unsupported on this env. Default: none.
    /// ADR 0063 §6; ticket 0777.
    fn preset_path(&self, _patch_stem: &str, _preset_name: &str) -> Option<PathBuf> {
        None
    }
    /// List preset names for the patch identified by `patch_stem`.
    /// Default: empty.
    fn list_presets(&mut self, _patch_stem: &str) -> Vec<String> {
        Vec::new()
    }
    /// Read a preset's `PersistedSettings`. `Ok(None)` for missing.
    /// Default: not supported.
    fn load_preset(&mut self, _path: &Path) -> std::io::Result<Option<PersistedSettings>> {
        Ok(None)
    }
    /// Write `settings` as a preset at `path`. Default: no-op.
    fn save_preset(
        &mut self,
        _path: &Path,
        _settings: &PersistedSettings,
    ) -> std::io::Result<()> {
        Ok(())
    }
}

/// Schema version embedded in every preset envelope (ticket 0777).
pub const PRESET_SCHEMA_VERSION: u32 = 1;

/// On-disk envelope for a saved preset. Carries a schema version and
/// a snapshot of [`PersistedSettings`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PresetEnvelope {
    pub v: u32,
    pub settings: PersistedSettings,
}

impl PresetEnvelope {
    pub fn new(settings: PersistedSettings) -> Self {
        Self {
            v: PRESET_SCHEMA_VERSION,
            settings,
        }
    }
}

/// Default preset library root: `$XDG_DATA_HOME/patches/presets` (or
/// `~/.local/share/patches/presets`). Returns `None` if neither var is
/// set, which lets `preset_path` default to "unsupported" cleanly.
pub fn default_preset_library_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| {
                let mut p = PathBuf::from(h);
                p.push(".local/share");
                p
            })
        })?;
    let mut p = base;
    p.push("patches");
    p.push("presets");
    Some(p)
}

/// JSON-on-disk preset I/O against [`default_preset_library_dir`]. Both
/// shells route through this; `Env` impls just delegate.
pub fn xdg_preset_path(patch_stem: &str, preset_name: &str) -> Option<PathBuf> {
    let mut p = default_preset_library_dir()?;
    p.push(patch_stem);
    p.push(format!("{preset_name}.json"));
    Some(p)
}

pub fn xdg_list_presets(patch_stem: &str) -> Vec<String> {
    let dir = match default_preset_library_dir() {
        Some(d) => {
            let mut p = d;
            p.push(patch_stem);
            p
        }
        None => return Vec::new(),
    };
    let mut out: Vec<String> = match std::fs::read_dir(&dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let p = e.path();
                if p.extension().is_some_and(|x| x == "json") {
                    p.file_stem().map(|s| s.to_string_lossy().into_owned())
                } else {
                    None
                }
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    out.sort();
    out
}

pub fn xdg_save_preset(path: &Path, settings: &PersistedSettings) -> std::io::Result<()> {
    let envelope = PresetEnvelope::new(settings.clone());
    let json = serde_json::to_string_pretty(&envelope)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("preset: {e}")))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, json)
}

pub fn xdg_load_preset(path: &Path) -> std::io::Result<Option<PersistedSettings>> {
    let bytes = match std::fs::read_to_string(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let env: PresetEnvelope = serde_json::from_str(&bytes)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("preset: {e}")))?;
    if env.v != PRESET_SCHEMA_VERSION {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("preset schema v{} (expected v{})", env.v, PRESET_SCHEMA_VERSION),
        ));
    }
    Ok(Some(env.settings))
}

/// Schema version embedded in every sidecar envelope. Bump when the
/// shape of [`PersistedSettings`] changes incompatibly so older readers
/// can refuse to load rather than silently misinterpret bytes.
pub const SIDECAR_SCHEMA_VERSION: u32 = 1;

/// On-disk envelope for the Ratatui sidecar (ticket 0775). Carries a
/// schema version alongside the settings payload.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SidecarEnvelope {
    pub v: u32,
    pub settings: PersistedSettings,
}

impl SidecarEnvelope {
    pub fn new(settings: PersistedSettings) -> Self {
        Self {
            v: SIDECAR_SCHEMA_VERSION,
            settings,
        }
    }
}

/// Persistable + derived plugin model.
#[derive(Default)]
pub struct Controller {
    // Persistable.
    pub file_path: Option<PathBuf>,
    pub dsl_source: String,
    pub module_paths: Vec<PathBuf>,
    pub tap_opts: HashMap<String, TapDisplayOpts>,
    pub window_size: Option<(u32, u32)>,
    /// Host-control values keyed by manifest name (ADR 0057, ticket
    /// 0813a). Persisted via `PersistedSettings` and cross-referenced
    /// against `host_control_manifest` on every ingress: entries whose
    /// name doesn't appear in the current manifest are dropped with a
    /// status-log diagnostic. The filter is deferred until the first
    /// post-`StateLoad` compile lands a manifest — until then the map
    /// is held verbatim.
    pub host_controls: HashMap<String, f32>,

    // Derived / live.
    pub registry: Registry,
    pub status_log: VecDeque<String>,
    pub diagnostic_view: DiagnosticView,
    pub halt: Option<HaltInfoSnapshot>,
    pub taps: Vec<TapSummary>,
    pub module_names: Vec<String>,
    /// Preset names available for the current patch identity. Refreshed
    /// on `LoadPath` and after `SavePreset`. Ticket 0777.
    pub preset_names: Vec<String>,
    /// Most recent host-control manifest seen at compile time. Empty
    /// before the first successful compile. Plugin shells diff this
    /// against their `HostControlRegistry` after each compile to drive
    /// CLAP parameter publication (ADR 0057 §6, ticket 0811).
    pub host_control_manifest: Arc<HostControlManifest>,
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
            Action::Browse => self.apply_browse(env),
            Action::Reload => self.apply_reload(env),
            Action::LoadPath(path) => self.load_path(path, "Loaded", env),
            Action::AddModulePath => self.apply_add_module_path(env),
            Action::AddModulePathDirect(dir) => self.add_module_path(dir, env),
            Action::RemoveModulePath(idx) => self.apply_remove_module_path(idx),
            Action::Rescan => self.apply_rescan(env),
            Action::SetTapOpt { name, opt } => self.apply_set_tap_opt(name, opt),
            Action::SetWindowSize(w, h) => self.apply_set_window_size(w, h),
            Action::Activate => self.apply_activate(env),
            Action::Deactivate => self.apply_deactivate(),
            Action::SavePreset { name } => self.apply_save_preset(name, env),
            Action::LoadPreset { name } => self.apply_load_preset(name, env),
            Action::StateLoad { identity, settings } => self.apply_state_load(identity, settings),
            Action::HaltObserved(observed) => self.apply_halt_observed(observed),
            Action::DiagnosticsDrained(lines) => self.apply_diagnostics_drained(lines),
        }
    }

    fn apply_browse(&mut self, env: &mut dyn Env) -> StateDelta {
        match env.pick_file() {
            Some(path) => self.load_path(path, "Loaded", env),
            None => StateDelta::default(),
        }
    }

    fn apply_reload(&mut self, env: &mut dyn Env) -> StateDelta {
        match self.file_path.clone() {
            Some(path) => self.load_path(path, "Reloaded", env),
            None => StateDelta::default(),
        }
    }

    fn apply_add_module_path(&mut self, env: &mut dyn Env) -> StateDelta {
        match env.pick_folder() {
            Some(dir) => self.add_module_path(dir, env),
            None => StateDelta::default(),
        }
    }

    fn apply_remove_module_path(&mut self, idx: usize) -> StateDelta {
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

    fn apply_rescan(&mut self, env: &mut dyn Env) -> StateDelta {
        let probe = env.probe_paths(&self.module_paths, &self.registry);
        let errors_suffix = if probe.errors.is_empty() {
            String::new()
        } else {
            format!(", {} errors", probe.errors.len())
        };
        self.push_status(format!(
            "Rescan: {} added, {} replaced, {} unchanged{}",
            probe.added.len(),
            probe.replaced.len(),
            probe.unchanged.len(),
            errors_suffix,
        ));
        for line in &probe.errors {
            self.push_status(line.clone());
        }
        // Predicted post-restart names = current registry ∪ added.
        let mut names: Vec<String> =
            self.registry.module_names().map(|s| s.to_string()).collect();
        for n in &probe.added {
            if !names.iter().any(|x| x == n) {
                names.push(n.clone());
            }
        }
        names.sort();
        self.module_names = names;
        if probe.is_actionable() {
            self.diagnostic_view = DiagnosticView::default();
            StateDelta {
                requires_restart: true,
                snapshot_changed: true,
                ..Default::default()
            }
        } else {
            StateDelta {
                snapshot_changed: true,
                ..Default::default()
            }
        }
    }

    fn apply_set_tap_opt(&mut self, name: String, opt: TapOpt) -> StateDelta {
        if name.is_empty() {
            // Unnamed taps are not addressable for opts. Drop
            // silently (ADR 0063 §5; ticket 0773).
            return StateDelta::default();
        }
        let entry = self.tap_opts.entry(name).or_default();
        let before = *entry;
        match opt {
            TapOpt::SpectrumFftSize(n) => entry.spectrum_fft_size = n,
            TapOpt::ScopeDecimation(d) => entry.scope_decimation = d,
            TapOpt::ScopeWindowSamples(w) => entry.scope_window_samples = w,
            TapOpt::ScopeSnap(b) => {
                entry.scope_snap = if b { ScopeMode::Snap } else { ScopeMode::Free }
            }
            TapOpt::SpectrumHeatmap(b) => {
                entry.spectrum_heatmap = if b { SpectrumRender::Heatmap } else { SpectrumRender::Curves }
            }
        }
        let changed = *entry != before;
        StateDelta {
            persistable_changed: changed,
            snapshot_changed: changed,
            ..Default::default()
        }
    }

    fn apply_set_window_size(&mut self, w: u32, h: u32) -> StateDelta {
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

    fn apply_activate(&mut self, env: &mut dyn Env) -> StateDelta {
        let (registry, scan) = env.reset_and_scan(&self.module_paths);
        self.registry = registry;
        self.module_names = scan.module_names;
        if !self.module_paths.is_empty() {
            self.push_status(format!("Module scan: {}", scan.summary));
            for line in scan.details {
                self.push_status(line);
            }
        }
        let plan_recompile = if self.dsl_source.is_empty() {
            false
        } else {
            self.compile_current_source(env)
        };
        StateDelta {
            snapshot_changed: true,
            plan_recompile,
            ..Default::default()
        }
    }

    /// Compile `self.dsl_source` against the current registry and absorb
    /// the result into derived state. Returns `true` on success (caller
    /// uses this to set `plan_recompile`). Errors land in the diagnostic
    /// view and status log.
    fn compile_current_source(&mut self, env: &mut dyn Env) -> bool {
        match env.compile_and_push_plan(
            &self.dsl_source,
            self.file_path.as_deref(),
            &self.registry,
        ) {
            Ok(success) => {
                self.absorb_compile_success(success);
                true
            }
            Err(failure) => {
                self.diagnostic_view = failure.view;
                self.push_status(format!("Error: {}", failure.message));
                false
            }
        }
    }

    fn absorb_compile_success(&mut self, success: CompileSuccess) {
        self.taps = success.taps;
        self.host_control_manifest = success.host_control_manifest;
        self.reconcile_host_controls_with_manifest();
        self.diagnostic_view = DiagnosticView::default();
        if !success.warnings.is_empty() {
            self.diagnostic_view.diagnostics.extend(success.warnings);
        }
    }

    fn apply_deactivate(&mut self) -> StateDelta {
        // Clear derived/live fields the audio side will rebuild
        // on next Activate. Persistable fields (file_path,
        // dsl_source, module_paths, tap_opts, window_size)
        // survive — they drive the next Activate.
        self.registry = Registry::default();
        self.taps.clear();
        self.module_names.clear();
        self.halt = None;
        self.diagnostic_view = DiagnosticView::default();
        self.host_control_manifest = Arc::new(Vec::new());
        StateDelta {
            snapshot_changed: true,
            ..Default::default()
        }
    }

    fn apply_save_preset(&mut self, name: String, env: &mut dyn Env) -> StateDelta {
        let stem = self.patch_stem();
        let Some(path) = env.preset_path(&stem, &name) else {
            self.push_status("Presets unsupported on this env");
            return StateDelta::default();
        };
        let settings = self.persisted_settings();
        match env.save_preset(&path, &settings) {
            Ok(()) => {
                self.preset_names = env.list_presets(&stem);
                self.push_status(format!("Saved preset {name}"));
                StateDelta {
                    snapshot_changed: true,
                    ..Default::default()
                }
            }
            Err(e) => {
                self.push_status(format!("Save preset {name} failed: {e}"));
                StateDelta::default()
            }
        }
    }

    fn apply_load_preset(&mut self, name: String, env: &mut dyn Env) -> StateDelta {
        let stem = self.patch_stem();
        let Some(path) = env.preset_path(&stem, &name) else {
            self.push_status("Presets unsupported on this env");
            return StateDelta::default();
        };
        match env.load_preset(&path) {
            Ok(Some(settings)) => {
                self.absorb_persisted_settings(settings);
                self.push_status(format!("Loaded preset {name}"));
                StateDelta {
                    persistable_changed: true,
                    snapshot_changed: true,
                    ..Default::default()
                }
            }
            Ok(None) => {
                self.push_status(format!("Preset {name} not found"));
                StateDelta::default()
            }
            Err(e) => {
                self.push_status(format!("Load preset {name} failed: {e}"));
                StateDelta::default()
            }
        }
    }

    /// Apply `settings` to the persistable fields and reconcile host
    /// controls against the current manifest.
    fn absorb_persisted_settings(&mut self, settings: PersistedSettings) {
        self.module_paths = settings.module_paths;
        self.tap_opts = settings.tap_opts;
        self.window_size = settings.window_size;
        self.host_controls = settings.host_controls;
        self.reconcile_host_controls_with_manifest();
    }

    fn apply_state_load(
        &mut self,
        identity: PatchIdentity,
        settings: PersistedSettings,
    ) -> StateDelta {
        self.file_path = identity.file_path;
        self.dsl_source = identity.dsl_source;
        self.absorb_persisted_settings(settings);
        StateDelta {
            snapshot_changed: true,
            ..Default::default()
        }
    }

    fn apply_halt_observed(&mut self, observed: Option<HaltInfoSnapshot>) -> StateDelta {
        let same = match (&observed, &self.halt) {
            (None, None) => true,
            (Some(a), Some(b)) => a.slot == b.slot && a.module_name == b.module_name,
            _ => false,
        };
        if same {
            return StateDelta::default();
        }
        if let Some(info) = &observed {
            let first = info.payload.lines().next().unwrap_or("");
            self.push_status(format!(
                "Engine halted: module {:?} (slot {}) panicked: {} — reload the patch to recover.",
                info.module_name, info.slot, first,
            ));
        }
        self.halt = observed;
        StateDelta {
            snapshot_changed: true,
            ..Default::default()
        }
    }

    fn apply_diagnostics_drained(&mut self, lines: Vec<String>) -> StateDelta {
        if lines.is_empty() {
            return StateDelta::default();
        }
        for line in lines {
            self.push_status(line);
        }
        StateDelta {
            snapshot_changed: true,
            ..Default::default()
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
            preset_names: self.preset_names.clone(),
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
        // Scan-before-compile: ADR 0061. Ensures any FFI module
        // referenced by the patch is in the registry before parse.
        if !self.module_paths.is_empty() {
            let scan = env.scan_into(&self.module_paths, &mut self.registry);
            if !scan.summary.starts_with("0 loaded, 0 replaced, 0 skipped, 0 errors") {
                self.push_status(format!("Module scan: {}", scan.summary));
                for line in scan.details {
                    self.push_status(line);
                }
            }
            self.module_names = scan.module_names;
        }
        match env.compile_and_push_plan(&self.dsl_source, self.file_path.as_deref(), &self.registry)
        {
            Ok(success) => {
                self.taps = success.taps;
                self.host_control_manifest = success.host_control_manifest;
                self.reconcile_host_controls_with_manifest();
                self.diagnostic_view = DiagnosticView::default();
                if !success.warnings.is_empty() {
                    self.diagnostic_view.diagnostics.extend(success.warnings);
                }
                delta.plan_recompile = true;
                self.push_status(success_msg);
                // Refresh available preset list under this patch stem.
                self.preset_names = env.list_presets(&self.patch_stem());
                // Sidecar restore (ADR 0063 §5; ticket 0776). Failure
                // is non-fatal — surface in the status log and carry on
                // with the (possibly default) current settings.
                if let Some(file_path) = self.file_path.clone() {
                    if let Some(sidecar) = env.sidecar_path(&file_path) {
                        match env.load_sidecar(&sidecar) {
                            Ok(Some(settings)) => {
                                self.module_paths = settings.module_paths;
                                self.tap_opts = settings.tap_opts;
                                self.window_size = settings.window_size;
                                self.host_controls = settings.host_controls;
                                self.reconcile_host_controls_with_manifest();
                                self.push_status(format!(
                                    "Loaded sidecar: {}",
                                    sidecar.display()
                                ));
                            }
                            Ok(None) => {}
                            Err(e) => {
                                self.push_status(format!(
                                    "Sidecar load failed ({}): {e}",
                                    sidecar.display()
                                ));
                            }
                        }
                    }
                }
            }
            Err(failure) => {
                self.diagnostic_view = failure.view;
                self.push_status(format!("Error: {}", failure.message));
            }
        }
        delta
    }

    /// Patch identity stem used to group presets. Falls back to a
    /// stable placeholder if no file_path is set.
    fn patch_stem(&self) -> String {
        self.file_path
            .as_ref()
            .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "_unknown".to_string())
    }

    /// Drop persisted host-control values whose name is absent from the
    /// current manifest, logging a single status line listing them
    /// (ADR 0057, ticket 0813a). Called after every ingress that
    /// touches either side of the (cache, manifest) pair.
    ///
    /// No-op when the manifest is empty — the cache is held until the
    /// first successful compile lands one. This avoids dropping every
    /// entry on `StateLoad` (which arrives before any compile has
    /// produced a manifest in this session).
    fn reconcile_host_controls_with_manifest(&mut self) {
        if self.host_control_manifest.is_empty() || self.host_controls.is_empty() {
            return;
        }
        let known: std::collections::HashSet<&str> = self
            .host_control_manifest
            .iter()
            .map(|d| d.name.as_str())
            .collect();
        let mut unknown: Vec<String> = self
            .host_controls
            .keys()
            .filter(|n| !known.contains(n.as_str()))
            .cloned()
            .collect();
        if unknown.is_empty() {
            return;
        }
        unknown.sort();
        for name in &unknown {
            self.host_controls.remove(name);
        }
        self.push_status(format!(
            "Dropped {} host-control value(s) not in current manifest: {}",
            unknown.len(),
            unknown.join(", "),
        ));
    }

    /// Snapshot of the current persistable settings, for shells that
    /// need to write a sidecar on dirty (ADR 0063 §5; ticket 0776).
    pub fn persisted_settings(&self) -> PersistedSettings {
        PersistedSettings {
            host_controls: self.host_controls.clone(),
            tap_opts: self.tap_opts.clone(),
            window_size: self.window_size,
            module_paths: self.module_paths.clone(),
        }
    }

    fn add_module_path(&mut self, dir: PathBuf, env: &mut dyn Env) -> StateDelta {
        if self.module_paths.iter().any(|p| p == &dir) {
            return StateDelta::default();
        }
        self.push_status(format!(
            "Added module path: {} (press Rescan to load)",
            dir.display()
        ));
        self.module_paths.push(dir);
        // Probe preview — surface ABI / load errors immediately, and
        // preview module additions without restarting (ADR 0061).
        let probe = env.probe_paths(&self.module_paths, &self.registry);
        if probe.is_actionable() {
            self.push_status(format!(
                "Preview: {} added, {} replaced, {} unchanged",
                probe.added.len(),
                probe.replaced.len(),
                probe.unchanged.len(),
            ));
        }
        for line in &probe.errors {
            self.push_status(line.clone());
        }
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
        probe: RescanProbe,
        compiled_sources: Vec<String>,
        sidecar_for: Option<PathBuf>,
        sidecar_payload: Option<PersistedSettings>,
        saved_sidecars: Vec<(PathBuf, PersistedSettings)>,
        preset_root: Option<PathBuf>,
        presets: HashMap<PathBuf, PersistedSettings>,
        preset_listings: HashMap<String, Vec<String>>,
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
                    host_control_manifest: Arc::new(Vec::new()),
                })
            } else {
                Err(CompileFailure {
                    message: "boom".into(),
                    view: DiagnosticView::default(),
                })
            }
        }
        fn probe_paths(&mut self, _paths: &[PathBuf], _registry: &Registry) -> RescanProbe {
            self.probe.clone()
        }
        fn scan_into(&mut self, _paths: &[PathBuf], _registry: &mut Registry) -> ScanDetails {
            self.scan.clone()
        }
        fn reset_and_scan(&mut self, _paths: &[PathBuf]) -> (Registry, ScanDetails) {
            (Registry::default(), self.scan.clone())
        }
        fn sidecar_path(&self, _patch_path: &Path) -> Option<PathBuf> {
            self.sidecar_for.clone()
        }
        fn load_sidecar(&mut self, _path: &Path) -> std::io::Result<Option<PersistedSettings>> {
            Ok(self.sidecar_payload.clone())
        }
        fn save_sidecar(
            &mut self,
            path: &Path,
            settings: &PersistedSettings,
        ) -> std::io::Result<()> {
            self.saved_sidecars.push((path.to_path_buf(), settings.clone()));
            Ok(())
        }
        fn preset_path(&self, stem: &str, name: &str) -> Option<PathBuf> {
            let root = self.preset_root.as_ref()?;
            Some(root.join(stem).join(format!("{name}.json")))
        }
        fn list_presets(&mut self, stem: &str) -> Vec<String> {
            self.preset_listings.get(stem).cloned().unwrap_or_default()
        }
        fn save_preset(
            &mut self,
            path: &Path,
            settings: &PersistedSettings,
        ) -> std::io::Result<()> {
            self.presets.insert(path.to_path_buf(), settings.clone());
            Ok(())
        }
        fn load_preset(&mut self, path: &Path) -> std::io::Result<Option<PersistedSettings>> {
            Ok(self.presets.get(path).cloned())
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
    fn rescan_actionable_probe_requires_restart() {
        let mut env = ok_env();
        env.probe = RescanProbe {
            added: vec!["Gain".into()],
            ..Default::default()
        };
        let mut c = Controller::new();
        let d = c.apply(Action::Rescan, &mut env);
        assert!(d.requires_restart);
        assert!(d.snapshot_changed);
        assert!(c.module_names.iter().any(|n| n == "Gain"));
        let last = c.status_log.back().cloned().unwrap_or_default();
        assert!(last.contains("Rescan:"));
    }

    #[test]
    fn rescan_idempotent_probe_skips_restart() {
        let mut env = ok_env();
        env.probe = RescanProbe {
            unchanged: vec!["Gain".into()],
            ..Default::default()
        };
        let mut c = Controller::new();
        let d = c.apply(Action::Rescan, &mut env);
        assert!(!d.requires_restart);
        assert!(d.snapshot_changed);
    }

    #[test]
    fn rescan_surfaces_probe_errors() {
        let mut env = ok_env();
        env.probe = RescanProbe {
            errors: vec!["  skip /tmp/x: ABI mismatch".into()],
            ..Default::default()
        };
        let mut c = Controller::new();
        c.apply(Action::Rescan, &mut env);
        assert!(c
            .status_log
            .iter()
            .any(|s| s.contains("ABI mismatch")));
    }

    #[test]
    fn add_module_path_runs_probe_preview() {
        let mut env = ok_env();
        env.probe = RescanProbe {
            added: vec!["Gain".into()],
            ..Default::default()
        };
        let mut c = Controller::new();
        c.apply(Action::AddModulePathDirect("/tmp/m".into()), &mut env);
        assert!(c.status_log.iter().any(|s| s.starts_with("Preview:")));
    }

    #[test]
    fn load_path_runs_scan_before_compile() {
        let path = PathBuf::from("/tmp/x.patches");
        let mut env = ok_env();
        env.files.insert(path.clone(), "x".into());
        env.scan = ScanDetails {
            summary: "1 loaded, 0 replaced, 0 skipped, 0 errors".into(),
            details: vec![],
            module_names: vec!["Gain".into()],
        };
        let mut c = Controller::new();
        c.module_paths.push("/tmp/m".into());
        c.apply(Action::LoadPath(path), &mut env);
        // Module scan ran (status pushed), then compile.
        assert!(c.status_log.iter().any(|s| s.starts_with("Module scan:")));
        assert_eq!(env.compiled_sources, vec!["x".to_string()]);
        assert_eq!(c.module_names, vec!["Gain".to_string()]);
    }

    #[test]
    fn load_path_skips_scan_when_no_module_paths() {
        let path = PathBuf::from("/tmp/x.patches");
        let mut env = ok_env();
        env.files.insert(path.clone(), "x".into());
        let mut c = Controller::new();
        c.apply(Action::LoadPath(path), &mut env);
        assert!(!c.status_log.iter().any(|s| s.starts_with("Module scan:")));
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
    fn set_tap_opt_marks_persistable_when_changed() {
        let mut env = ok_env();
        let mut c = Controller::new();
        let d = c.apply(
            Action::SetTapOpt {
                name: "kick".into(),
                opt: TapOpt::SpectrumFftSize(2048),
            },
            &mut env,
        );
        assert!(d.persistable_changed);
        assert_eq!(c.tap_opts.get("kick").unwrap().spectrum_fft_size, 2048);

        // Re-apply same value — no change.
        let d2 = c.apply(
            Action::SetTapOpt {
                name: "kick".into(),
                opt: TapOpt::SpectrumFftSize(2048),
            },
            &mut env,
        );
        assert!(!d2.persistable_changed);
    }

    #[test]
    fn set_tap_opt_with_empty_name_is_dropped() {
        let mut env = ok_env();
        let mut c = Controller::new();
        let d = c.apply(
            Action::SetTapOpt {
                name: String::new(),
                opt: TapOpt::SpectrumFftSize(2048),
            },
            &mut env,
        );
        assert_eq!(d, StateDelta::default());
        assert!(c.tap_opts.is_empty());
    }

    #[test]
    fn unimplemented_host_events_return_default_delta() {
        let mut env = ok_env();
        let mut c = Controller::new();
        for a in [
            Action::DiagnosticsDrained(Vec::new()),
            Action::HaltObserved(None),
        ] {
            assert_eq!(c.apply(a, &mut env), StateDelta::default());
        }
    }

    fn fake_halt(slot: usize) -> patches_engine::HaltInfoSnapshot {
        patches_engine::HaltInfoSnapshot {
            slot,
            module_name: format!("M{slot}"),
            payload: "boom".to_string(),
        }
    }

    #[test]
    fn halt_observed_first_time_pushes_status() {
        let mut env = ok_env();
        let mut c = Controller::new();
        let d = c.apply(Action::HaltObserved(Some(fake_halt(7))), &mut env);
        assert!(d.snapshot_changed);
        assert!(c.halt.is_some());
        let last = c.status_log.back().cloned().unwrap_or_default();
        assert!(last.contains("slot 7"));
    }

    #[test]
    fn halt_observed_same_slot_is_idempotent() {
        let mut env = ok_env();
        let mut c = Controller::new();
        c.apply(Action::HaltObserved(Some(fake_halt(0))), &mut env);
        let len_before = c.status_log.len();
        let d = c.apply(Action::HaltObserved(Some(fake_halt(0))), &mut env);
        assert_eq!(d, StateDelta::default());
        assert_eq!(c.status_log.len(), len_before);
    }

    #[test]
    fn halt_observed_clear_resets_state() {
        let mut env = ok_env();
        let mut c = Controller::new();
        c.apply(Action::HaltObserved(Some(fake_halt(0))), &mut env);
        let d = c.apply(Action::HaltObserved(None), &mut env);
        assert!(d.snapshot_changed);
        assert!(c.halt.is_none());
    }

    #[test]
    fn diagnostics_drained_pushes_each_render() {
        let mut env = ok_env();
        let mut c = Controller::new();
        let d = c.apply(Action::DiagnosticsDrained(Vec::new()), &mut env);
        assert_eq!(d, StateDelta::default());
        assert!(c.status_log.is_empty());
    }

    #[test]
    fn deactivate_clears_derived_fields_keeps_persistable() {
        let mut env = ok_env();
        let mut c = Controller::new();
        c.file_path = Some("/tmp/a.patches".into());
        c.dsl_source = "x".into();
        c.module_paths.push("/tmp/m".into());
        c.taps.push(TapSummary::default());
        c.module_names.push("Gain".into());
        let d = c.apply(Action::Deactivate, &mut env);
        assert!(d.snapshot_changed);
        assert!(c.taps.is_empty());
        assert!(c.module_names.is_empty());
        // Persistable survives.
        assert_eq!(c.dsl_source, "x");
        assert_eq!(c.module_paths.len(), 1);
        assert!(c.file_path.is_some());
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
        let identity = PatchIdentity {
            file_path: Some("/tmp/x.patches".into()),
            dsl_source: "fresh".into(),
        };
        let settings = PersistedSettings {
            module_paths: vec!["/tmp/m".into()],
            tap_opts: Default::default(),
            window_size: Some((800, 600)),
            host_controls: Default::default(),
        };
        let d = c.apply(Action::StateLoad { identity, settings }, &mut env);
        assert!(d.snapshot_changed);
        assert!(!d.persistable_changed);
        assert_eq!(c.dsl_source, "fresh");
        assert_eq!(c.file_path.as_deref(), Some(std::path::Path::new("/tmp/x.patches")));
        assert_eq!(c.module_paths, vec![PathBuf::from("/tmp/m")]);
        assert_eq!(c.window_size, Some((800, 600)));
    }

    #[test]
    fn load_path_restores_sidecar_when_present() {
        let path = PathBuf::from("/tmp/x.patches");
        let sidecar = PathBuf::from("/tmp/x.patches.state");
        let mut env = ok_env();
        env.files.insert(path.clone(), "src".into());
        env.sidecar_for = Some(sidecar.clone());
        env.sidecar_payload = Some(PersistedSettings {
            host_controls: Default::default(),
            tap_opts: {
                let mut m = HashMap::new();
                m.insert(
                    "kick".to_string(),
                    TapDisplayOpts {
                        spectrum_fft_size: 4096,
                        ..Default::default()
                    },
                );
                m
            },
            window_size: Some((1024, 768)),
            module_paths: vec![PathBuf::from("/tmp/m")],
        });
        let mut c = Controller::new();
        c.apply(Action::LoadPath(path), &mut env);
        assert_eq!(c.window_size, Some((1024, 768)));
        assert_eq!(c.tap_opts.get("kick").unwrap().spectrum_fft_size, 4096);
        assert_eq!(c.module_paths, vec![PathBuf::from("/tmp/m")]);
        assert!(c
            .status_log
            .iter()
            .any(|s| s.starts_with("Loaded sidecar:")));
    }

    #[test]
    fn load_path_with_missing_sidecar_keeps_defaults() {
        let path = PathBuf::from("/tmp/x.patches");
        let mut env = ok_env();
        env.files.insert(path.clone(), "src".into());
        env.sidecar_for = Some(PathBuf::from("/tmp/x.patches.state"));
        // sidecar_payload is None → load_sidecar returns Ok(None).
        let mut c = Controller::new();
        c.window_size = Some((100, 100));
        c.apply(Action::LoadPath(path), &mut env);
        // Settings unchanged by missing sidecar.
        assert_eq!(c.window_size, Some((100, 100)));
        assert!(!c
            .status_log
            .iter()
            .any(|s| s.starts_with("Loaded sidecar:")));
    }

    #[test]
    fn persisted_settings_mirrors_controller_fields() {
        let mut c = Controller::new();
        c.window_size = Some((1, 2));
        c.module_paths.push("/tmp/m".into());
        c.tap_opts.insert("k".into(), TapDisplayOpts::default());
        let s = c.persisted_settings();
        assert_eq!(s.window_size, Some((1, 2)));
        assert_eq!(s.module_paths, vec![PathBuf::from("/tmp/m")]);
        assert!(s.tap_opts.contains_key("k"));
    }

    #[test]
    fn save_preset_writes_to_resolved_path_and_updates_listing() {
        let mut env = ok_env();
        env.preset_root = Some(PathBuf::from("/lib"));
        let mut c = Controller::new();
        c.file_path = Some("/tmp/A.patches".into());
        c.tap_opts.insert(
            "kick".into(),
            TapDisplayOpts {
                spectrum_fft_size: 4096,
                ..Default::default()
            },
        );
        // After save, FakeEnv reports the new listing.
        env.preset_listings
            .insert("A".into(), vec!["bright".into()]);
        let d = c.apply(
            Action::SavePreset {
                name: "bright".into(),
            },
            &mut env,
        );
        assert!(d.snapshot_changed);
        let stored_at = PathBuf::from("/lib/A/bright.json");
        let saved = env.presets.get(&stored_at).expect("preset stored");
        assert_eq!(saved.tap_opts.get("kick").unwrap().spectrum_fft_size, 4096);
        assert_eq!(c.preset_names, vec!["bright".to_string()]);
    }

    #[test]
    fn cross_patch_load_keeps_unmatched_names_for_later_reconciliation() {
        let mut env = ok_env();
        env.preset_root = Some(PathBuf::from("/lib"));
        // Save against patch A, then load against patch B (same preset
        // name lookup uses B's stem, so we write the preset directly
        // into FakeEnv at B's resolved path to simulate a manual move).
        let mut payload = PersistedSettings::default();
        payload.tap_opts.insert(
            "kick".into(),
            TapDisplayOpts {
                spectrum_fft_size: 4096,
                ..Default::default()
            },
        );
        env.presets
            .insert(PathBuf::from("/lib/B/bright.json"), payload);

        let mut c = Controller::new();
        c.file_path = Some("/tmp/B.patches".into());
        let d = c.apply(
            Action::LoadPreset {
                name: "bright".into(),
            },
            &mut env,
        );
        assert!(d.persistable_changed);
        // Unmatched-by-current-manifest names land verbatim; later
        // reconciliation prunes them. For now we just hold them.
        assert_eq!(c.tap_opts.get("kick").unwrap().spectrum_fft_size, 4096);
    }

    #[test]
    fn load_preset_missing_is_status_logged_and_no_op() {
        let mut env = ok_env();
        env.preset_root = Some(PathBuf::from("/lib"));
        let mut c = Controller::new();
        c.file_path = Some("/tmp/A.patches".into());
        let d = c.apply(
            Action::LoadPreset {
                name: "nope".into(),
            },
            &mut env,
        );
        assert_eq!(d, StateDelta::default());
        assert!(c
            .status_log
            .iter()
            .any(|s| s.contains("Preset nope not found")));
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

    fn manifest_with_names(names: &[&str]) -> Arc<HostControlManifest> {
        use patches_dsl::host_control_manifest::{
            HostControlDescriptor, HostControlKind, HostControlParamMap,
        };
        use patches_dsl::provenance::Provenance;
        use patches_dsl::Span;
        Arc::new(
            names
                .iter()
                .enumerate()
                .map(|(slot, name)| HostControlDescriptor {
                    slot,
                    name: (*name).to_string(),
                    kind: HostControlKind::Knob,
                    params: HostControlParamMap::new(),
                    source: Provenance::root(Span::synthetic()),
                })
                .collect(),
        )
    }

    #[test]
    fn state_load_with_empty_manifest_keeps_unknown_values() {
        // Until the first compile lands a manifest, the cache is held
        // verbatim — applying a fresh project state must not strip any
        // entries.
        let mut env = ok_env();
        let mut c = Controller::new();
        let mut hc = HashMap::new();
        hc.insert("freq".to_string(), 0.5);
        hc.insert("res".to_string(), 0.3);
        let _ = c.apply(
            Action::StateLoad {
                identity: PatchIdentity::default(),
                settings: PersistedSettings {
                    host_controls: hc.clone(),
                    ..Default::default()
                },
            },
            &mut env,
        );
        assert_eq!(c.host_controls, hc);
        assert!(
            !c.status_log
                .iter()
                .any(|line| line.contains("not in current manifest")),
            "must not log a drop-diagnostic without a manifest",
        );
    }

    #[test]
    fn state_load_filters_against_published_manifest() {
        let mut env = ok_env();
        let mut c = Controller::new();
        c.host_control_manifest = manifest_with_names(&["freq"]);
        let mut hc = HashMap::new();
        hc.insert("freq".to_string(), 0.5);
        hc.insert("stale".to_string(), 0.9);
        hc.insert("ghost".to_string(), 0.1);
        let _ = c.apply(
            Action::StateLoad {
                identity: PatchIdentity::default(),
                settings: PersistedSettings {
                    host_controls: hc,
                    ..Default::default()
                },
            },
            &mut env,
        );
        assert_eq!(c.host_controls.len(), 1);
        assert_eq!(c.host_controls.get("freq").copied(), Some(0.5));
        let line = c
            .status_log
            .iter()
            .find(|l| l.contains("not in current manifest"))
            .cloned()
            .expect("drop-diagnostic logged");
        assert!(line.contains("ghost"), "diagnostic lists dropped names: {line}");
        assert!(line.contains("stale"), "diagnostic lists dropped names: {line}");
    }

    #[test]
    fn compile_success_filters_stale_cache_against_new_manifest() {
        // A cache populated before any manifest survives the StateLoad
        // gate (no manifest yet). The first successful compile must
        // then prune entries the new manifest doesn't cover.
        let path = PathBuf::from("/tmp/p.patches");
        let mut env = FakeEnv {
            compile_ok: true,
            ..Default::default()
        };
        env.files.insert(path.clone(), "src".into());
        let mut c = Controller::new();
        c.host_controls.insert("freq".to_string(), 0.5);
        c.host_controls.insert("stale".to_string(), 0.7);
        // Stub compile_and_push_plan returns an empty manifest; redirect
        // through a manifest-bearing wrapper.
        struct ManifestEnv<'a> {
            inner: &'a mut FakeEnv,
            manifest: Arc<HostControlManifest>,
        }
        impl<'a> Env for ManifestEnv<'a> {
            fn pick_file(&mut self) -> Option<PathBuf> { self.inner.pick_file() }
            fn pick_folder(&mut self) -> Option<PathBuf> { self.inner.pick_folder() }
            fn read_file(&mut self, p: &Path) -> std::io::Result<String> { self.inner.read_file(p) }
            fn compile_and_push_plan(
                &mut self,
                source: &str,
                file_path: Option<&Path>,
                registry: &Registry,
            ) -> Result<CompileSuccess, CompileFailure> {
                let mut s = self.inner.compile_and_push_plan(source, file_path, registry)?;
                s.host_control_manifest = self.manifest.clone();
                Ok(s)
            }
            fn probe_paths(&mut self, p: &[PathBuf], r: &Registry) -> RescanProbe {
                self.inner.probe_paths(p, r)
            }
            fn scan_into(&mut self, p: &[PathBuf], r: &mut Registry) -> ScanDetails {
                self.inner.scan_into(p, r)
            }
            fn reset_and_scan(&mut self, p: &[PathBuf]) -> (Registry, ScanDetails) {
                self.inner.reset_and_scan(p)
            }
            fn sidecar_path(&self, p: &Path) -> Option<PathBuf> { self.inner.sidecar_path(p) }
            fn load_sidecar(&mut self, p: &Path) -> std::io::Result<Option<PersistedSettings>> {
                self.inner.load_sidecar(p)
            }
            fn save_sidecar(&mut self, p: &Path, s: &PersistedSettings) -> std::io::Result<()> {
                self.inner.save_sidecar(p, s)
            }
            fn preset_path(&self, stem: &str, name: &str) -> Option<PathBuf> {
                self.inner.preset_path(stem, name)
            }
            fn list_presets(&mut self, stem: &str) -> Vec<String> {
                self.inner.list_presets(stem)
            }
            fn save_preset(&mut self, p: &Path, s: &PersistedSettings) -> std::io::Result<()> {
                self.inner.save_preset(p, s)
            }
            fn load_preset(&mut self, p: &Path) -> std::io::Result<Option<PersistedSettings>> {
                self.inner.load_preset(p)
            }
        }
        let manifest = manifest_with_names(&["freq"]);
        let mut env = ManifestEnv { inner: &mut env, manifest };
        let _ = c.apply(Action::LoadPath(path), &mut env);
        assert_eq!(c.host_controls.len(), 1);
        assert!(c.host_controls.contains_key("freq"));
        let line = c
            .status_log
            .iter()
            .find(|l| l.contains("not in current manifest"))
            .cloned()
            .expect("drop-diagnostic logged after compile");
        assert!(line.contains("stale"), "diagnostic mentions dropped name: {line}");
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
