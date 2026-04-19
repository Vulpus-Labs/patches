//! Plugin scanner: unified discovery and registration of FFI plugin
//! bundles across every `Registry` consumer (player, CLAP, LSP).
//!
//! See ADR 0044 §4.

use std::path::{Path, PathBuf};

use patches_core::ModuleShape;
use patches_registry::{ModuleBuilder, RegisterOutcome, Registry};
use serde::{Deserialize, Serialize};

use crate::loader::{load_plugin, DylibModuleBuilder};

/// The platform-specific shared library extension.
#[cfg(target_os = "macos")]
pub const LIB_EXT: &str = "dylib";
#[cfg(target_os = "linux")]
pub const LIB_EXT: &str = "so";
#[cfg(target_os = "windows")]
pub const LIB_EXT: &str = "dll";

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
/// Each path may be a directory (enumerated for `LIB_EXT` files) or a
/// concrete shared-library file. `scan` is idempotent against an already
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
                .map(|s| s == LIB_EXT)
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
        let name = builder.describe(&default_shape).module_name.to_string();
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
        let has_ext = path.extension().and_then(|s| s.to_str()).map(|s| s == LIB_EXT).unwrap_or(false);
        if !has_ext {
            continue;
        }
        match load_plugin(&path) {
            Ok(builders) => {
                let shape = ModuleShape::default();
                for builder in builders {
                    let name = builder.describe(&shape).module_name.to_string();
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
}
