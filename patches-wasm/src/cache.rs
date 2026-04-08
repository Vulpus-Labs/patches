//! AOT compilation cache for WASM plugins.
//!
//! Compiled WASM modules are serialized to `.wasmcache` files alongside the
//! source `.wasm` files. On subsequent loads, the cached artifact is used if
//! it is newer than the `.wasm` file and deserializes successfully.

use std::path::Path;
use std::sync::Arc;

use wasmtime::{Engine, Module};

/// Load a compiled WASM module, using a cached `.wasmcache` file if available.
///
/// Cache logic:
/// 1. If `<name>.wasmcache` exists and is newer than the `.wasm` file, try
///    to deserialize it.
/// 2. If deserialization fails (e.g. wasmtime version change), fall through
///    to compilation.
/// 3. After compiling from source, serialize and write to `.wasmcache`.
///    Write failures are non-fatal.
pub fn load_or_compile(engine: &Arc<Engine>, wasm_path: &Path) -> Result<Module, String> {
    let cache_path = wasm_path.with_extension("wasmcache");

    // Try loading from cache
    if let Some(cached) = try_load_cached(engine, wasm_path, &cache_path) {
        return Ok(cached);
    }

    // Compile from source
    let module = Module::from_file(engine, wasm_path)
        .map_err(|e| format!("failed to compile WASM {}: {e}", wasm_path.display()))?;

    // Write cache (best-effort)
    if let Ok(serialized) = module.serialize() {
        let _ = std::fs::write(&cache_path, serialized);
    }

    Ok(module)
}

fn try_load_cached(engine: &Arc<Engine>, wasm_path: &Path, cache_path: &Path) -> Option<Module> {
    // Check cache file exists
    let cache_meta = std::fs::metadata(cache_path).ok()?;
    let wasm_meta = std::fs::metadata(wasm_path).ok()?;

    // Check cache is newer than source
    let cache_modified = cache_meta.modified().ok()?;
    let wasm_modified = wasm_meta.modified().ok()?;
    if cache_modified < wasm_modified {
        return None;
    }

    // Try to deserialize
    let cached_bytes = std::fs::read(cache_path).ok()?;
    // Safety: wasmtime validates the serialized format internally, including
    // version checks and checksums. A corrupted cache will fail deserialization
    // rather than producing UB.
    unsafe { Module::deserialize(engine, &cached_bytes) }.ok()
}
