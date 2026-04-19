//! Loader tests for multi-module bundles.

use std::path::PathBuf;

use patches_ffi::loader::load_plugin;

fn workspace_target_debug() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.push("target");
    path.push("debug");
    path
}

fn dylib(name: &str) -> PathBuf {
    let mut path = workspace_target_debug();
    #[cfg(target_os = "macos")]
    path.push(format!("lib{name}.dylib"));
    #[cfg(target_os = "linux")]
    path.push(format!("lib{name}.so"));
    #[cfg(target_os = "windows")]
    path.push(format!("{name}.dll"));
    path
}

#[test]
fn drums_bundle_loads_eight_builders_sharing_library() {
    let path = dylib("test_drums_bundle");
    let builders = load_plugin(&path).expect("failed to load drums bundle");
    assert_eq!(builders.len(), 8, "expected 8 drum modules in bundle");
    // All builders must share a single Arc<Library> — at least builders.len()
    // strong references live in the vec, plus one is the one we observe.
    let count = builders[0].library_strong_count();
    assert!(
        count >= builders.len(),
        "expected Arc::strong_count >= {}, got {count}",
        builders.len(),
    );
}

#[test]
fn abi_v1_plugin_rejected() {
    let path = dylib("test_old_abi_plugin");
    let err = match load_plugin(&path) {
        Ok(_) => panic!("v1 plugin should be rejected"),
        Err(e) => e,
    };
    assert!(
        err.contains("ABI version") || err.to_lowercase().contains("version"),
        "error should mention version mismatch; got: {err}",
    );
}
