//! Ratatui-side `Env` impl for `patches-plugin-common::Controller`.
//!
//! ADR 0063 §1, ticket 0772. The Ratatui shell drives the same
//! `Controller` as the CLAP shell. Side-effects the controller cannot
//! perform itself land here: file picking (no-op without a picker UI),
//! file reads, DSL compile + plan dispatch through `HostRuntime`, and
//! module-path scans through the FFI plugin scanner.
//!
//! Sidecar persistence is not part of the `Env` trait yet (ticket 0775
//! adds the methods). When it lands, the Ratatui impl reads/writes a
//! sibling `<patch>.patches.state` JSON file.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use patches_diagnostics::RenderedDiagnostic;
use patches_dsl::manifest::Manifest;
use patches_host::{HostRuntime, InMemorySource};
use patches_plugin_common::{
    default_global_config_path, xdg_list_presets, xdg_load_preset, xdg_preset_path,
    xdg_save_preset, CompileFailure, CompileSuccess, DiagnosticView, Env, GlobalConfig,
    PersistedSettings, RescanProbe, ScanDetails, SidecarEnvelope, TapSummary,
    SIDECAR_SCHEMA_VERSION,
};
use patches_core::registry::Registry;

/// Side-channel data the controller's `Env` trait does not surface but
/// the player's outer loop needs (for the file watcher and the rich
/// `TapEntry` rendering). Updated each compile.
#[derive(Default)]
pub struct EnvSideChannel {
    pub last_manifest: Option<Manifest>,
    pub last_dependencies: Vec<PathBuf>,
    pub last_expand_warnings: Vec<String>,
    /// Bundle-dir / module paths already merged into the live registry
    /// (canonicalised when possible). Lets [`Env::scan_into`] skip
    /// previously-scanned paths so patch reloads don't re-scan, while
    /// [`Action::AddBundleDir`](patches_plugin_common::Action::AddBundleDir)
    /// still hot-loads freshly added directories.
    pub scanned_paths: HashSet<PathBuf>,
}

pub struct RatatuiEnv<'a> {
    pub runtime: &'a mut HostRuntime,
    pub side: &'a mut EnvSideChannel,
}

impl<'a> Env for RatatuiEnv<'a> {
    fn pick_file(&mut self) -> Option<PathBuf> {
        // No file picker in the TUI; loads come from the CLI arg.
        None
    }
    fn pick_folder(&mut self) -> Option<PathBuf> {
        None
    }
    fn read_file(&mut self, path: &Path) -> std::io::Result<String> {
        std::fs::read_to_string(path)
    }
    fn compile_and_push_plan(
        &mut self,
        source: &str,
        file_path: Option<&Path>,
        registry: &Registry,
    ) -> Result<CompileSuccess, CompileFailure> {
        let mut src = InMemorySource::new(source.to_string());
        if let Some(path) = file_path {
            src = src.with_master_path(path.to_path_buf());
        }
        match self.runtime.compile_and_push_blocking(&src, registry) {
            Ok(loaded) => {
                let taps = project_manifest_taps(&loaded.manifest);
                let warnings: Vec<RenderedDiagnostic> = loaded
                    .layering_warnings
                    .iter()
                    .map(RenderedDiagnostic::from_layering_warning)
                    .collect();
                self.side.last_dependencies = loaded.dependencies.clone();
                self.side.last_manifest = Some(loaded.manifest.clone());
                self.side.last_expand_warnings = loaded
                    .expand_warnings
                    .iter()
                    .map(|w| format!("dsl warning: {w}"))
                    .collect();
                Ok(CompileSuccess {
                    taps,
                    warnings,
                    host_control_manifest: std::sync::Arc::new(
                        loaded.host_control_manifest.clone(),
                    ),
                })
            }
            Err(e) => {
                let view = DiagnosticView {
                    diagnostics: e.to_rendered_diagnostics(),
                    source_map: Some(e.source_map.clone()),
                };
                Err(CompileFailure {
                    message: format!("{e}"),
                    view,
                })
            }
        }
    }
    fn probe_paths(&mut self, paths: &[PathBuf], registry: &Registry) -> RescanProbe {
        let mut probe = RescanProbe::default();
        if paths.is_empty() {
            return probe;
        }
        let mut throwaway = Registry::new();
        let report = patches_ffi::PluginScanner::new(paths.to_vec()).scan(&mut throwaway);
        let names: Vec<String> = throwaway.module_names().map(|s| s.to_string()).collect();
        for name in names {
            let candidate = throwaway.module_version(&name).unwrap_or(0);
            match registry.module_version(&name) {
                None => probe.added.push(name),
                Some(live) if live < candidate => probe.replaced.push(name),
                _ => probe.unchanged.push(name),
            }
        }
        for (path, err) in &report.errors {
            probe.errors.push(format!("  error {}: {err}", path.display()));
        }
        for skip in &report.skipped {
            if let patches_ffi::SkipReason::AbiMismatch { expected, found, path } = skip {
                probe.errors.push(format!(
                    "  skip {}: ABI mismatch (host {expected}, plugin {found})",
                    path.display()
                ));
            }
        }
        probe
    }
    fn scan_into(&mut self, paths: &[PathBuf], registry: &mut Registry) -> ScanDetails {
        // Dedup against paths already scanned this session (startup
        // scan + prior AddBundleDir actions). Re-scanning would emit
        // "skip lower-version" lines on every patch reload otherwise.
        let unseen: Vec<PathBuf> = paths
            .iter()
            .filter(|p| !self.side.scanned_paths.contains(*p))
            .cloned()
            .collect();
        if unseen.is_empty() {
            let mut module_names: Vec<String> =
                registry.module_names().map(|s| s.to_string()).collect();
            module_names.sort();
            return ScanDetails {
                summary: "0 loaded, 0 replaced, 0 skipped, 0 errors".into(),
                details: Vec::new(),
                module_names,
            };
        }
        let details = scan_into_registry(&unseen, registry);
        for p in &unseen {
            self.side
                .scanned_paths
                .insert(std::fs::canonicalize(p).unwrap_or_else(|_| p.clone()));
            self.side.scanned_paths.insert(p.clone());
        }
        details
    }
    fn reset_and_scan(&mut self, paths: &[PathBuf]) -> (Registry, ScanDetails) {
        let mut registry = patches_modules::default_registry();
        let details = scan_into_registry(paths, &mut registry);
        (registry, details)
    }

    fn sidecar_path(&self, patch_path: &Path) -> Option<PathBuf> {
        Some(sidecar_for(patch_path))
    }

    fn load_sidecar(&mut self, path: &Path) -> std::io::Result<Option<PersistedSettings>> {
        let primary = std::fs::read_to_string(path);
        let bytes = match primary {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Try the XDG fallback location for this patch path
                // before declaring the sidecar absent.
                let fallback = xdg_fallback_for(path);
                match std::fs::read_to_string(&fallback) {
                    Ok(b) => b,
                    Err(e2) if e2.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                    Err(e2) => return Err(e2),
                }
            }
            Err(e) => return Err(e),
        };
        let env: SidecarEnvelope = serde_json::from_str(&bytes).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, format!("sidecar: {e}"))
        })?;
        if env.v != SIDECAR_SCHEMA_VERSION {
            // Future-incompatible schema — refuse rather than misread.
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("sidecar schema v{} (expected v{})", env.v, SIDECAR_SCHEMA_VERSION),
            ));
        }
        Ok(Some(env.settings))
    }

    fn global_config_path(&self) -> Option<PathBuf> {
        global_config_path()
    }

    fn load_global_config(&mut self) -> std::io::Result<Option<GlobalConfig>> {
        let Some(path) = global_config_path() else {
            return Ok(None);
        };
        let bytes = match std::fs::read_to_string(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        match GlobalConfig::from_toml_str(&bytes) {
            Ok(cfg) => Ok(Some(cfg)),
            Err(e) => Err(e.into()),
        }
    }

    fn save_global_config(&mut self, cfg: &GlobalConfig) -> std::io::Result<()> {
        let path = global_config_path().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "global config path unavailable on this platform",
            )
        })?;
        let serialised = cfg
            .to_toml_string()
            .map_err(std::io::Error::from)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Atomic write: tmp + rename so a crash mid-write can't corrupt
        // the live settings.toml. The tmp is sibling to the target so
        // `rename` stays on one filesystem.
        let mut tmp = path.clone();
        match tmp.file_name() {
            Some(n) => {
                let mut name = n.to_os_string();
                name.push(".tmp");
                tmp.set_file_name(name);
            }
            None => return Err(std::io::Error::other("global config path has no filename")),
        }
        std::fs::write(&tmp, serialised.as_bytes())?;
        std::fs::rename(&tmp, &path)
    }

    fn preset_path(&self, patch_stem: &str, preset_name: &str) -> Option<PathBuf> {
        xdg_preset_path(patch_stem, preset_name)
    }
    fn list_presets(&mut self, patch_stem: &str) -> Vec<String> {
        xdg_list_presets(patch_stem)
    }
    fn load_preset(&mut self, path: &Path) -> std::io::Result<Option<PersistedSettings>> {
        xdg_load_preset(path)
    }
    fn save_preset(&mut self, path: &Path, settings: &PersistedSettings) -> std::io::Result<()> {
        xdg_save_preset(path, settings)
    }

    fn save_sidecar(
        &mut self,
        path: &Path,
        settings: &PersistedSettings,
    ) -> std::io::Result<()> {
        // ADR 0075 §"Persistence boundary": sidecars never carry bundle
        // dirs. The legacy field stays in the serde shape for back-
        // compat with existing files; we just write empty.
        let mut scrubbed = settings.clone();
        scrubbed.module_paths.clear();
        let envelope = SidecarEnvelope::new(scrubbed);
        let json = serde_json::to_string_pretty(&envelope).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, format!("sidecar: {e}"))
        })?;
        match std::fs::write(path, &json) {
            Ok(()) => Ok(()),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem
                ) =>
            {
                let fallback = xdg_fallback_for(path);
                if let Some(parent) = fallback.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&fallback, json)
            }
            Err(e) => Err(e),
        }
    }
}

/// OS-native global-config location (ADR 0075). Thin wrapper over the
/// shared helper in `patches_plugin_common`; both shells must hit the
/// same path so writes from one are visible to the other.
pub(crate) fn global_config_path() -> Option<PathBuf> {
    default_global_config_path()
}

/// Sidecar location adjacent to a `.patches` file:
/// `<patch>.patches.state`. The caller's `patch_path` already points at
/// `<patch>.patches`; we just append the suffix.
pub(crate) fn sidecar_for(patch_path: &Path) -> PathBuf {
    let mut p = patch_path.as_os_str().to_owned();
    p.push(".state");
    PathBuf::from(p)
}

/// XDG state-dir fallback when the patch's directory isn't writable.
/// Keyed by a stable hash of the absolute path so two patches with the
/// same name in different directories don't collide.
pub(crate) fn xdg_fallback_for(sidecar_path: &Path) -> PathBuf {
    let abs = sidecar_path
        .canonicalize()
        .unwrap_or_else(|_| sidecar_path.to_path_buf());
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    use std::hash::{Hash, Hasher};
    abs.hash(&mut hasher);
    let key = format!("{:016x}", hasher.finish());
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| {
                let mut p = PathBuf::from(h);
                p.push(".local/state");
                p
            })
        })
        .unwrap_or_else(|| PathBuf::from("."));
    let mut p = base;
    p.push("patches");
    p.push(format!("{key}.patches.state"));
    p
}

fn scan_into_registry(paths: &[PathBuf], registry: &mut Registry) -> ScanDetails {
    // Resolve all four ADR 0075 tiers on every (re)scan, not just at startup.
    // `with_global_dirs` folds in `PATCHES_PLUGIN_PATH` (tier 1) and the
    // OS-default bundle dir (tier 4) on top of `paths` (the controller's
    // module_paths, carrying the settings.toml bundle_dirs). Tier 3 is passed
    // empty so a hot-reload re-scan doesn't re-persist env/default entries back
    // into module_paths (which is saved as bundle_dirs).
    let scanner = patches_ffi::PluginScanner::with_global_dirs(paths.to_vec(), &[]);
    let (summary, details) = if scanner.paths.is_empty() {
        (String::new(), Vec::new())
    } else {
        let report = scanner.scan(registry);
        (report.summary(), scan_detail_lines(&report))
    };
    let mut module_names: Vec<String> =
        registry.module_names().map(|s| s.to_string()).collect();
    module_names.sort();
    ScanDetails {
        summary,
        details,
        module_names,
    }
}

fn scan_detail_lines(report: &patches_ffi::ScanReport) -> Vec<String> {
    use patches_ffi::SkipReason;
    let mut out = Vec::new();
    for (path, err) in &report.errors {
        out.push(format!("  error {}: {err}", path.display()));
    }
    for skip in &report.skipped {
        match skip {
            SkipReason::AbiMismatch { expected, found, path } => out.push(format!(
                "  skip {}: ABI mismatch (host {expected}, plugin {found})",
                path.display()
            )),
            SkipReason::LowerVersion { name, existing, candidate, path } => out.push(format!(
                "  skip {}: {name} v{candidate} <= existing v{existing}",
                path.display()
            )),
            SkipReason::DuplicateInBundle { name, path } => out.push(format!(
                "  skip {}: duplicate {name} in bundle",
                path.display()
            )),
        }
    }
    out
}

fn project_manifest_taps(manifest: &Manifest) -> Vec<TapSummary> {
    manifest
        .iter()
        .map(|d| TapSummary {
            name: d.name.clone(),
            slot: d.slot,
            kind: if d.components.len() == 1 {
                d.components[0].as_str().to_string()
            } else {
                "compound".to_string()
            },
            components: d
                .components
                .iter()
                .map(|c| c.as_str().to_string())
                .collect(),
        })
        .collect()
}

#[cfg(test)]
mod sidecar_tests {
    use super::*;
    use patches_host::HostBuilder;
    use patches_plugin_common::TapDisplayOpts;

    /// Build a minimal Env wrapping a throwaway runtime so we can call
    /// the sidecar trait methods without standing up a full audio stack.
    fn with_env<F: FnOnce(&mut RatatuiEnv)>(f: F) {
        let env = patches_core::AudioEnvironment {
            sample_rate: 48_000.0,
            poly_voices: 2,
            periodic_update_interval: patches_core::BASE_PERIODIC_UPDATE_INTERVAL,
            hosted: false,
        };
        let mut runtime = HostBuilder::new().build(env).expect("runtime");
        let mut side = EnvSideChannel::default();
        let mut e = RatatuiEnv {
            runtime: &mut runtime,
            side: &mut side,
        };
        f(&mut e);
    }

    fn sample_settings() -> PersistedSettings {
        let mut tap_opts = std::collections::HashMap::new();
        tap_opts.insert(
            "kick".to_string(),
            TapDisplayOpts {
                spectrum_fft_size: 2048,
                ..Default::default()
            },
        );
        PersistedSettings {
            host_controls: Default::default(),
            tap_opts,
            window_size: Some((1024, 768)),
            module_paths: vec![PathBuf::from("/tmp/m")],
        }
    }

    #[test]
    fn sidecar_path_appends_state_suffix() {
        let p = Path::new("/tmp/foo.patches");
        with_env(|e| {
            assert_eq!(
                e.sidecar_path(p).unwrap(),
                PathBuf::from("/tmp/foo.patches.state"),
            );
        });
    }

    #[test]
    fn json_round_trip_through_filesystem() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let path = dir.path().join("a.patches.state");
        let original = sample_settings();
        with_env(|e| {
            e.save_sidecar(&path, &original).expect("save");
            let loaded = e.load_sidecar(&path).expect("load").expect("some");
            // ADR 0075: sidecars never carry bundle dirs; the
            // legacy `module_paths` field is scrubbed on save.
            let mut expected = original.clone();
            expected.module_paths.clear();
            assert_eq!(loaded, expected);
        });
    }

    #[test]
    fn missing_sidecar_returns_ok_none() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let path = dir.path().join("missing.patches.state");
        with_env(|e| {
            assert!(e.load_sidecar(&path).expect("load").is_none());
        });
    }

    #[test]
    fn read_only_parent_falls_back_to_xdg() {
        // Force XDG_STATE_HOME to a writable temp dir; point the
        // sidecar at a read-only directory and verify save_sidecar
        // succeeds by writing into the fallback path.
        let xdg = tempfile::tempdir().expect("xdg");
        let ro = tempfile::tempdir().expect("ro");
        let ro_path = ro.path().to_path_buf();
        let primary = ro_path.join("a.patches.state");

        let mut perms = std::fs::metadata(&ro_path).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&ro_path, perms).expect("chmod ro");

        // Override XDG_STATE_HOME for the duration of this test.
        let prev = std::env::var_os("XDG_STATE_HOME");
        // Safety: setting a process-wide env var. Tests in this module
        // run sequentially within a single binary, but cargo runs tests
        // in parallel by default — gate on a serialised mutex via
        // file-based locking on the tempdir. For now accept the race;
        // the only other test that touches XDG is this one.
        unsafe {
            std::env::set_var("XDG_STATE_HOME", xdg.path());
        }

        let settings = sample_settings();
        with_env(|e| {
            e.save_sidecar(&primary, &settings).expect("save fallback");
            // Original location did not get the file (ro).
            assert!(!primary.exists());
            // Fallback under xdg/patches/<hash>.patches.state exists.
            let fb = xdg_fallback_for(&primary);
            assert!(fb.exists(), "fallback {} not found", fb.display());
        });

        // Restore permissions so tempdir cleanup works.
        let mut perms = std::fs::metadata(&ro_path).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        std::fs::set_permissions(&ro_path, perms).expect("restore");

        // Restore env.
        unsafe {
            match prev {
                Some(v) => std::env::set_var("XDG_STATE_HOME", v),
                None => std::env::remove_var("XDG_STATE_HOME"),
            }
        }
    }
}
