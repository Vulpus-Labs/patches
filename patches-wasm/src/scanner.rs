//! WASM plugin scanner: discovers `.wasm` files in a directory, loads each,
//! and registers them in the module registry.

use std::path::Path;
use std::sync::Arc;

use wasmtime::Engine;

use patches_core::modules::ModuleShape;
use patches_core::registries::{ModuleBuilder, Registry};

use crate::loader::{load_wasm_plugin, WasmModuleBuilder};

/// Scan a directory for `.wasm` plugin files and load each one.
///
/// Returns a vec of `(module_name, builder)` pairs for successfully loaded
/// plugins, or per-file error strings for failures. A broken plugin does not
/// prevent other plugins from loading.
pub fn scan_wasm_plugins(
    engine: &Arc<Engine>,
    dir: &Path,
) -> Vec<Result<(String, WasmModuleBuilder), String>> {
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

        let is_file = path.metadata().map(|m| m.is_file()).unwrap_or(false);
        if !is_file {
            continue;
        }

        let has_ext = path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext == "wasm")
            .unwrap_or(false);
        if !has_ext {
            continue;
        }

        match load_wasm_plugin(engine, &path) {
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

/// Scan a directory for WASM plugins and register each successful one in the
/// registry. Returns collected error messages for any plugins that failed
/// to load.
///
/// If a WASM module and a native module have the same name, the last one
/// registered wins (standard HashMap behavior).
pub fn register_wasm_plugins(
    engine: &Arc<Engine>,
    dir: &Path,
    registry: &mut Registry,
) -> Vec<String> {
    let results = scan_wasm_plugins(engine, dir);
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
        let engine = Arc::new(Engine::default());
        let results = scan_wasm_plugins(&engine, Path::new("/nonexistent/wasm_plugins"));
        assert_eq!(results.len(), 1);
        assert!(results[0].is_err());
    }

    #[test]
    fn scan_empty_directory() {
        let engine = Arc::new(Engine::default());
        let dir = std::env::temp_dir().join("patches_test_empty_wasm_plugins");
        let _ = std::fs::create_dir_all(&dir);
        let results = scan_wasm_plugins(&engine, &dir);
        assert!(results.is_empty());
        let _ = std::fs::remove_dir(&dir);
    }
}
