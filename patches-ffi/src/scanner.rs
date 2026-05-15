//! Plugin scanner: unified discovery and registration of FFI plugin
//! bundles across every `Registry` consumer (player, CLAP, LSP).
//!
//! See ADR 0044 §4.

use std::path::{Path, PathBuf};

use patches_core::ModuleShape;
use patches_core::registry::{ModuleBuilder, RegisterOutcome, Registry};
use serde::{Deserialize, Serialize};

use crate::loader::{load_plugin, DylibModuleBuilder};

/// The platform-specific shared library extension. Retained for callers
/// (tests, host integration) that need to locate raw build artefacts in
/// `target/<profile>/`. The scanner itself filters on [`PLUGIN_EXT`].
#[cfg(target_os = "macos")]
pub const LIB_EXT: &str = "dylib";
#[cfg(target_os = "linux")]
pub const LIB_EXT: &str = "so";
#[cfg(target_os = "windows")]
pub const LIB_EXT: &str = "dll";

/// The Patches extension module extension. Bundles ship as `*.pxm`
/// regardless of host platform — the underlying file is still a native
/// shared library, just renamed so that directory scans don't pick up
/// every unrelated `.dylib`/`.so`/`.dll` (e.g. `target/release/` debris).
pub const PLUGIN_EXT: &str = "pxm";

/// A single loaded or replaced module, recorded for reporting.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LoadedModule {
    pub name: String,
    pub version: u32,
    pub path: PathBuf,
}

/// A module whose builder replaced an earlier, lower-versioned one.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Replacement {
    pub name: String,
    pub from: u32,
    pub to: u32,
    pub path: PathBuf,
}

/// Reason a candidate entry was skipped without an outright error.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SkipReason {
    /// Candidate version is lower than (or equal to) the one already
    /// registered under this name.
    LowerVersion {
        name: String,
        existing: u32,
        candidate: u32,
        path: PathBuf,
    },
    /// ABI version on the plugin does not match the host.
    AbiMismatch {
        expected: u32,
        found: u32,
        path: PathBuf,
    },
    /// Two entries in one bundle share a module name — only the first is kept.
    DuplicateInBundle { name: String, path: PathBuf },
}

/// Structured outcome of a scan pass over the configured paths.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScanReport {
    pub loaded: Vec<LoadedModule>,
    pub replaced: Vec<Replacement>,
    pub skipped: Vec<SkipReason>,
    pub errors: Vec<(PathBuf, String)>,
}

impl ScanReport {
    /// Render a short one-line summary suitable for logs / GUI banners.
    pub fn summary(&self) -> String {
        format!(
            "{} loaded, {} replaced, {} skipped, {} errors",
            self.loaded.len(),
            self.replaced.len(),
            self.skipped.len(),
            self.errors.len(),
        )
    }
}

/// Discovers and registers FFI plugin bundles from a list of paths.
///
/// Each path may be a directory (enumerated for `*.pxm` files — see
/// [`PLUGIN_EXT`]) or a concrete shared-library file, which is loaded
/// regardless of extension. `scan` is idempotent against an already
/// populated registry: version compare decides whether each entry is
/// inserted, replaced, or skipped.
#[derive(Debug, Clone, Default)]
pub struct PluginScanner {
    pub paths: Vec<PathBuf>,
}

impl PluginScanner {
    pub fn new<I, P>(paths: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        Self { paths: paths.into_iter().map(Into::into).collect() }
    }

    /// Build a scanner whose path list is assembled from the four
    /// tiers documented in [ADR 0075] §"Scanner resolution order":
    ///
    /// 1. `PATCHES_PLUGIN_PATH` env var (colon-separated, deduplicated).
    /// 2. Caller-supplied `paths` (CLI `--module-path` for the player,
    ///    UI list for CLAP).
    /// 3. `global_bundle_dirs` from `GlobalConfig::bundle_dirs`.
    /// 4. `ProjectDirs::from("", "", "Patches").data_dir().join("bundles")`
    ///    iff that directory exists. The scanner never creates it; the
    ///    host installer (or user) drops bundles there explicitly.
    ///
    /// Entries are deduplicated across tiers using
    /// `std::fs::canonicalize` where possible (falling back to lexical
    /// equality when canonicalisation fails for e.g. a non-existent
    /// path), preserving the priority order of the first occurrence.
    /// Per the ticket scope this entry point is opt-in — existing
    /// callers using [`PluginScanner::new`] are unaffected, so unit
    /// tests stay deterministic and untouched by the new tiers.
    ///
    /// [ADR 0075]: ../../adr/0075-global-host-config-for-bundle-dirs.md
    pub fn with_global_dirs<I, P>(paths: I, global_bundle_dirs: &[PathBuf]) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        let mut merged: Vec<PathBuf> = Vec::new();
        let mut canon_seen: Vec<PathBuf> = Vec::new();
        let push = |p: PathBuf, merged: &mut Vec<PathBuf>, seen: &mut Vec<PathBuf>| {
            let key = std::fs::canonicalize(&p).unwrap_or_else(|_| p.clone());
            if seen.iter().any(|s| s == &key) {
                return;
            }
            seen.push(key);
            merged.push(p);
        };

        if let Ok(env_paths) = std::env::var("PATCHES_PLUGIN_PATH") {
            for s in env_paths.split(':').filter(|s| !s.is_empty()) {
                push(PathBuf::from(s), &mut merged, &mut canon_seen);
            }
        }
        for p in paths.into_iter() {
            push(p.into(), &mut merged, &mut canon_seen);
        }
        for p in global_bundle_dirs {
            push(p.clone(), &mut merged, &mut canon_seen);
        }
        if let Some(default) = default_bundle_dir() {
            if default.is_dir() {
                push(default, &mut merged, &mut canon_seen);
            }
        }

        Self { paths: merged }
    }

    /// Walk every configured path, loading bundles and updating `registry`
    /// with version-aware insertion. Never panics; every failure is recorded
    /// in the returned [`ScanReport`].
    pub fn scan(&self, registry: &mut Registry) -> ScanReport {
        let mut report = ScanReport::default();
        for path in &self.paths {
            self.scan_path(path, registry, &mut report);
        }
        report
    }

    fn scan_path(&self, path: &Path, registry: &mut Registry, report: &mut ScanReport) {
        let meta = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(e) => {
                report.errors.push((path.to_path_buf(), format!("stat failed: {e}")));
                return;
            }
        };
        if meta.is_file() {
            load_one(path, registry, report);
            return;
        }
        if !meta.is_dir() {
            report.errors.push((path.to_path_buf(), "not a file or directory".into()));
            return;
        }
        let entries = match std::fs::read_dir(path) {
            Ok(e) => e,
            Err(e) => {
                report.errors.push((path.to_path_buf(), format!("read_dir failed: {e}")));
                return;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    report.errors.push((path.to_path_buf(), format!("entry error: {e}")));
                    continue;
                }
            };
            let candidate = entry.path();
            let is_file = candidate.metadata().map(|m| m.is_file()).unwrap_or(false);
            if !is_file {
                continue;
            }
            let has_ext = candidate
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s == PLUGIN_EXT)
                .unwrap_or(false);
            if !has_ext {
                continue;
            }
            load_one(&candidate, registry, report);
        }
    }
}

fn load_one(path: &Path, registry: &mut Registry, report: &mut ScanReport) {
    let builders = match load_plugin(path) {
        Ok(b) => b,
        Err(e) => {
            report.errors.push((path.to_path_buf(), e));
            return;
        }
    };
    let default_shape = ModuleShape::default();
    let mut seen: Vec<String> = Vec::with_capacity(builders.len());
    for builder in builders {
        let version = builder.module_version();
        let name = builder.template().build_channels(default_shape.channels as u32).module_name.to_string();
        if seen.iter().any(|n| n == &name) {
            report.skipped.push(SkipReason::DuplicateInBundle {
                name,
                path: path.to_path_buf(),
            });
            continue;
        }
        seen.push(name.clone());
        match registry.register_builder_versioned(name.clone(), Box::new(builder), version) {
            RegisterOutcome::Inserted => report.loaded.push(LoadedModule {
                name,
                version,
                path: path.to_path_buf(),
            }),
            RegisterOutcome::Replaced { from, to } => report.replaced.push(Replacement {
                name,
                from,
                to,
                path: path.to_path_buf(),
            }),
            RegisterOutcome::Skipped { existing, candidate } => {
                report.skipped.push(SkipReason::LowerVersion {
                    name,
                    existing,
                    candidate,
                    path: path.to_path_buf(),
                });
            }
        }
    }
}

/// Resolve the OS-native default bundle dir
/// (`$DATA/Patches/bundles/`) via the `directories` crate. Returns
/// `None` when no home directory can be determined — equivalent to
/// the directory being absent.
pub fn default_bundle_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "Patches")
        .map(|d| d.data_dir().join("bundles"))
}

// ── Stdlib bundle discovery ──────────────────────────────────────────────────

/// Build a [`PluginScanner`] populated with the default search path for
/// the stdlib bundles (patches-vintage, patches-drums,
/// patches-fft-bundle — all shipped from the `patches-bundles`
/// repo). Ticket 0876 / [ADR 0073](../../adr/0073-monorepo-split-into-successor-repos.md).
///
/// Resolution order (first non-empty wins):
///
/// 1. `PATCHES_PLUGIN_PATH` env var, colon-separated. Honoured verbatim.
/// 2. `$EXE_DIR/plugins/` — the layout the release tarball / CLAP
///    bundle resources directory ships.
/// 3. Cargo dev paths: `$CWD/target/debug/` and
///    `$CWD/target/release/`. Either may be absent; existing ones
///    are added. Useful for `cargo run -p patches-player` from the
///    workspace root. The `.pxm` extension filter (see
///    [`PLUGIN_EXT`]) means raw cdylib build outputs are ignored —
///    bundles must be published as `*.pxm` to be picked up.
///
/// The returned scanner may have zero paths if none of the candidates
/// exist; callers should check [`PluginScanner::paths`] and decide
/// whether the absence is an error (see the player's `--no-stdlib`
/// flag).
pub fn stdlib_scanner() -> PluginScanner {
    if let Ok(env_paths) = std::env::var("PATCHES_PLUGIN_PATH") {
        let split: Vec<PathBuf> = env_paths
            .split(':')
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .collect();
        if !split.is_empty() {
            return PluginScanner::new(split);
        }
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("plugins"));
            // `cargo test` puts the test binary in target/<profile>/deps;
            // the cdylibs land alongside it in target/<profile>. Walk up
            // one level so dev test runs find them.
            if let Some(parent) = dir.parent() {
                candidates.push(parent.to_path_buf());
            }
            candidates.push(dir.to_path_buf());
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("target").join("debug"));
        candidates.push(cwd.join("target").join("release"));
    }

    let existing: Vec<PathBuf> = candidates
        .into_iter()
        .filter(|p| p.is_dir())
        .collect();
    PluginScanner::new(existing)
}

// ── Legacy shim ──────────────────────────────────────────────────────────────

/// Scan a directory for plugin shared libraries (legacy flat-list API).
///
/// Retained as a thin wrapper over [`PluginScanner`] so pre-E094 callers
/// continue to compile. New code should use [`PluginScanner::scan`].
pub fn scan_plugins(dir: &Path) -> Vec<Result<(String, DylibModuleBuilder), String>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => return vec![Err(format!("failed to read directory {}: {e}", dir.display()))],
    };
    let mut out = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                out.push(Err(format!("directory entry error: {e}")));
                continue;
            }
        };
        let path = entry.path();
        let is_file = path.metadata().map(|m| m.is_file()).unwrap_or(false);
        if !is_file {
            continue;
        }
        let has_ext = path.extension().and_then(|s| s.to_str()).map(|s| s == PLUGIN_EXT).unwrap_or(false);
        if !has_ext {
            continue;
        }
        match load_plugin(&path) {
            Ok(builders) => {
                let shape = ModuleShape::default();
                for builder in builders {
                    let name = builder.template().build_channels(shape.channels as u32).module_name.to_string();
                    out.push(Ok((name, builder)));
                }
            }
            Err(e) => out.push(Err(format!("{}: {e}", path.display()))),
        }
    }
    out
}

/// Scan and register — legacy wrapper. Prefer [`PluginScanner`].
pub fn register_plugins(dir: &Path, registry: &mut Registry) -> Vec<String> {
    let report = PluginScanner::new([dir.to_path_buf()]).scan(registry);
    report.errors.into_iter().map(|(p, e)| format!("{}: {e}", p.display())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_nonexistent_path() {
        let mut registry = Registry::new();
        let report = PluginScanner::new([PathBuf::from("/nonexistent/plugins")]).scan(&mut registry);
        assert_eq!(report.errors.len(), 1);
        assert!(report.loaded.is_empty());
    }

    #[test]
    fn scan_empty_directory() {
        let dir = std::env::temp_dir().join("patches_test_empty_plugins_v2");
        let _ = std::fs::create_dir_all(&dir);
        let mut registry = Registry::new();
        let report = PluginScanner::new([dir.clone()]).scan(&mut registry);
        assert!(report.loaded.is_empty());
        assert!(report.errors.is_empty());
        let _ = std::fs::remove_dir(&dir);
    }

    // ── with_global_dirs tier-merging ────────────────────────────────
    //
    // Tests serialise on the shared `PATCHES_PLUGIN_PATH` env var. A
    // process-wide mutex guards each test so concurrent test threads
    // don't see each other's env mutations. Each test sets the var
    // explicitly (including the empty-env case) and restores the prior
    // value on drop, so the suite is self-contained.

    use std::sync::Mutex;
    static ENV_GUARD: Mutex<()> = Mutex::new(());

    struct EnvScope {
        prev: Option<String>,
    }
    impl EnvScope {
        fn set(value: Option<&str>) -> Self {
            let prev = std::env::var("PATCHES_PLUGIN_PATH").ok();
            match value {
                Some(v) => std::env::set_var("PATCHES_PLUGIN_PATH", v),
                None => std::env::remove_var("PATCHES_PLUGIN_PATH"),
            }
            Self { prev }
        }
    }
    impl Drop for EnvScope {
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => std::env::set_var("PATCHES_PLUGIN_PATH", v),
                None => std::env::remove_var("PATCHES_PLUGIN_PATH"),
            }
        }
    }

    #[test]
    fn with_global_dirs_all_empty_yields_no_paths() {
        let _g = ENV_GUARD.lock().unwrap();
        let _e = EnvScope::set(Some(""));
        let scanner = PluginScanner::with_global_dirs(Vec::<PathBuf>::new(), &[]);
        // The default data dir may exist on a developer machine and be
        // a real `~/Library/Application Support/Patches/bundles/` etc.
        // The contract is "all four tiers empty ⇒ zero paths". Tier 4
        // only contributes when the dir exists, so we filter that
        // possibility here by asserting any included path is the
        // default bundle dir — never the env or caller tiers.
        let default = default_bundle_dir();
        for p in &scanner.paths {
            assert_eq!(Some(p), default.as_ref());
        }
    }

    #[test]
    fn with_global_dirs_env_takes_priority() {
        let _g = ENV_GUARD.lock().unwrap();
        let _e = EnvScope::set(Some("/tmp/patches_env_a:/tmp/patches_env_b"));
        let scanner = PluginScanner::with_global_dirs(
            vec![PathBuf::from("/tmp/patches_caller")],
            &[PathBuf::from("/tmp/patches_global")],
        );
        assert!(scanner.paths.len() >= 3);
        assert_eq!(scanner.paths[0], PathBuf::from("/tmp/patches_env_a"));
        assert_eq!(scanner.paths[1], PathBuf::from("/tmp/patches_env_b"));
        assert_eq!(scanner.paths[2], PathBuf::from("/tmp/patches_caller"));
        assert_eq!(scanner.paths[3], PathBuf::from("/tmp/patches_global"));
    }

    #[test]
    fn with_global_dirs_dedups_across_tiers() {
        let _g = ENV_GUARD.lock().unwrap();
        let _e = EnvScope::set(Some("/tmp/patches_dup"));
        let scanner = PluginScanner::with_global_dirs(
            vec![PathBuf::from("/tmp/patches_dup")],
            &[PathBuf::from("/tmp/patches_dup")],
        );
        let count = scanner
            .paths
            .iter()
            .filter(|p| *p == &PathBuf::from("/tmp/patches_dup"))
            .count();
        assert_eq!(count, 1, "duplicate path scanned more than once");
    }

    #[test]
    fn with_global_dirs_global_after_caller() {
        let _g = ENV_GUARD.lock().unwrap();
        let _e = EnvScope::set(Some(""));
        let scanner = PluginScanner::with_global_dirs(
            vec![PathBuf::from("/tmp/patches_caller")],
            &[PathBuf::from("/tmp/patches_global")],
        );
        let caller_idx = scanner
            .paths
            .iter()
            .position(|p| p == &PathBuf::from("/tmp/patches_caller"))
            .expect("caller path present");
        let global_idx = scanner
            .paths
            .iter()
            .position(|p| p == &PathBuf::from("/tmp/patches_global"))
            .expect("global path present");
        assert!(caller_idx < global_idx);
    }

    #[test]
    fn with_global_dirs_skips_missing_default_data_dir() {
        // Tier 4 is only included when the dir exists. We don't
        // assume anything about whether it exists on the test
        // machine — just that an explicit caller path is honoured
        // even when the default bundle dir is absent.
        let _g = ENV_GUARD.lock().unwrap();
        let _e = EnvScope::set(Some(""));
        let scanner = PluginScanner::with_global_dirs(
            vec![PathBuf::from("/tmp/patches_caller_only")],
            &[],
        );
        assert!(scanner.paths.contains(&PathBuf::from("/tmp/patches_caller_only")));
        // If the default exists it appears after the caller path; if
        // it doesn't, it's silently skipped (no error surfaced — the
        // scanner just has fewer entries).
        let default = default_bundle_dir();
        let default_present = scanner.paths.iter().any(|p| Some(p) == default.as_ref());
        if let Some(d) = default {
            assert_eq!(default_present, d.is_dir());
        } else {
            assert!(!default_present);
        }
    }
}
