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

use std::path::{Path, PathBuf};

use patches_diagnostics::RenderedDiagnostic;
use patches_dsl::manifest::Manifest;
use patches_host::{HostRuntime, InMemorySource};
use patches_plugin_common::{
    xdg_list_presets, xdg_load_preset, xdg_preset_path, xdg_save_preset, CompileFailure,
    CompileSuccess, DiagnosticView, Env, PersistedSettings, RescanProbe, ScanDetails,
    SidecarEnvelope, TapSummary, SIDECAR_SCHEMA_VERSION,
};
use patches_registry::Registry;

/// Side-channel data the controller's `Env` trait does not surface but
/// the player's outer loop needs (for the file watcher and the rich
/// `TapEntry` rendering). Updated each compile.
#[derive(Default)]
pub struct EnvSideChannel {
    pub last_manifest: Option<Manifest>,
    pub last_dependencies: Vec<PathBuf>,
    pub last_expand_warnings: Vec<String>,
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
    fn scan_into(&mut self, _paths: &[PathBuf], registry: &mut Registry) -> ScanDetails {
        // The player scans module paths once at startup (`common_setup`)
        // and treats that snapshot as authoritative — paths are fixed to
        // the CLI args and cannot change for the lifetime of the
        // process. The controller's pre-compile `scan_into` would
        // otherwise re-scan and log a "skip" line per already-loaded
        // module on every reload. Return an empty result and let the
        // existing registry stand.
        let mut module_names: Vec<String> =
            registry.module_names().map(|s| s.to_string()).collect();
        module_names.sort();
        ScanDetails {
            // Sentinel that the controller suppresses ("0 loaded, 0
            // replaced, 0 skipped, 0 errors"). Empty strings would log
            // as "Module scan: " — louder, not quieter.
            summary: "0 loaded, 0 replaced, 0 skipped, 0 errors".into(),
            details: Vec::new(),
            module_names,
        }
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
        let envelope = SidecarEnvelope::new(settings.clone());
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
    let (summary, details) = if paths.is_empty() {
        (String::new(), Vec::new())
    } else {
        let scanner = patches_ffi::PluginScanner::new(paths.to_vec());
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
            assert_eq!(loaded, original);
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
