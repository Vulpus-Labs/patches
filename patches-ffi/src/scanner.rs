//! Plugin scanner: discovers shared libraries in a directory, loads them,
//! and registers them in the module registry.

use std::path::Path;

use patches_core::ModuleShape;
use patches_registry::{ModuleBuilder, Registry};

use crate::loader::{load_plugin, DylibModuleBuilder};

/// The platform-specific shared library extension.
#[cfg(target_os = "macos")]
const LIB_EXT: &str = "dylib";
#[cfg(target_os = "linux")]
const LIB_EXT: &str = "so";
#[cfg(target_os = "windows")]
const LIB_EXT: &str = "dll";

/// Scan a directory for plugin shared libraries and load each one.
///
/// Returns a vec of `(module_name, builder)` pairs for successfully loaded
/// plugins, or per-file error strings for failures. A broken plugin does not
/// prevent other plugins from loading.
///
/// Symlinks are followed.
pub fn scan_plugins(dir: &Path) -> Vec<Result<(String, DylibModuleBuilder), String>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => return vec![Err(format!("failed to read directory {}: {e}", dir.display()))],
    };

    let mut results = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                results.push(Err(format!("directory entry error: {e}")));
                continue;
            }
        };

        let path = entry.path();

        // Follow symlinks: use metadata (not symlink_metadata)
        let is_file = path.metadata().map(|m| m.is_file()).unwrap_or(false);
        if !is_file {
            continue;
        }

        let has_ext = path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext == LIB_EXT)
            .unwrap_or(false);
        if !has_ext {
            continue;
        }

        match load_plugin(&path) {
            Ok(builder) => {
                let default_shape = ModuleShape::default();
                let desc = builder.describe(&default_shape);
                let name = desc.module_name.to_string();
                results.push(Ok((name, builder)));
            }
            Err(e) => {
                results.push(Err(format!("{}: {e}", path.display())));
            }
        }
    }

    results
}

/// Scan a directory for plugins and register each successful one in the
/// registry. Returns collected error messages for any plugins that failed
/// to load.
pub fn register_plugins(dir: &Path, registry: &mut Registry) -> Vec<String> {
    let results = scan_plugins(dir);
    let mut errors = Vec::new();
    for result in results {
        match result {
            Ok((name, builder)) => {
                registry.register_builder(name, Box::new(builder));
            }
            Err(e) => {
                errors.push(e);
            }
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_nonexistent_directory() {
        let results = scan_plugins(Path::new("/nonexistent/plugins"));
        assert_eq!(results.len(), 1);
        assert!(results[0].is_err());
    }

    #[test]
    fn scan_empty_directory() {
        let dir = std::env::temp_dir().join("patches_test_empty_plugins");
        let _ = std::fs::create_dir_all(&dir);
        let results = scan_plugins(&dir);
        assert!(results.is_empty());
        let _ = std::fs::remove_dir(&dir);
    }
}
