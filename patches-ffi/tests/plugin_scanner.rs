//! Integration tests for the plugin scanner.

use std::path::PathBuf;

use patches_core::modules::{InstanceId, ModuleShape, ParameterMap};
use patches_core::AudioEnvironment;
use patches_registry::Registry;
use patches_ffi::scanner::{register_plugins, scan_plugins};

fn gain_dylib_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.push("target");
    path.push("debug");
    #[cfg(target_os = "macos")]
    path.push("libtest_gain_plugin.dylib");
    #[cfg(target_os = "linux")]
    path.push("libtest_gain_plugin.so");
    #[cfg(target_os = "windows")]
    path.push("test_gain_plugin.dll");
    path
}

#[test]
fn scan_and_register_gain_plugin() {
    // Set up a temp directory with a symlink to the gain plugin
    let dir = std::env::temp_dir().join("patches_test_scan_plugins");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");

    let source = gain_dylib_path();
    assert!(source.exists(), "gain plugin dylib not found at {}", source.display());

    // Symlink the dylib into the scan directory
    #[cfg(unix)]
    std::os::unix::fs::symlink(&source, dir.join(source.file_name().unwrap()))
        .expect("symlink");
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&source, dir.join(source.file_name().unwrap()))
        .expect("symlink");

    // Scan
    let results = scan_plugins(&dir);
    let successes: Vec<_> = results.iter().filter(|r| r.is_ok()).collect();
    assert!(!successes.is_empty(), "expected at least one successful plugin load");

    // Register
    let mut registry = Registry::new();
    let errors = register_plugins(&dir, &mut registry);
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");

    // Verify the module is discoverable
    let names: Vec<_> = registry.module_names().collect();
    assert!(names.contains(&"Gain"), "Gain not found in registry; got: {names:?}");

    // Verify it's buildable
    let env = AudioEnvironment { sample_rate: 48000.0, poly_voices: 16, periodic_update_interval: 32, hosted: false };
    let shape = ModuleShape::default();
    let params = ParameterMap::new();
    let module = registry.create("Gain", &env, &shape, &params, InstanceId::next());
    assert!(module.is_ok(), "failed to create Gain from registry: {:?}", module.err());

    // Clean up
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn invalid_file_does_not_prevent_other_plugins() {
    let dir = std::env::temp_dir().join("patches_test_scan_mixed");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");

    // Write an invalid "plugin" file
    #[cfg(target_os = "macos")]
    let bad_name = "bad_plugin.dylib";
    #[cfg(target_os = "linux")]
    let bad_name = "bad_plugin.so";
    #[cfg(target_os = "windows")]
    let bad_name = "bad_plugin.dll";
    std::fs::write(dir.join(bad_name), b"not a real dylib").expect("write bad file");

    // Also symlink the good gain plugin
    let source = gain_dylib_path();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&source, dir.join(source.file_name().unwrap()))
        .expect("symlink");
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&source, dir.join(source.file_name().unwrap()))
        .expect("symlink");

    let mut registry = Registry::new();
    let errors = register_plugins(&dir, &mut registry);

    // One error for the bad file
    assert_eq!(errors.len(), 1, "expected 1 error, got: {errors:?}");

    // But the good plugin should still be registered
    let names: Vec<_> = registry.module_names().collect();
    assert!(names.contains(&"Gain"), "Gain not found despite bad sibling plugin");

    let _ = std::fs::remove_dir_all(&dir);
}
