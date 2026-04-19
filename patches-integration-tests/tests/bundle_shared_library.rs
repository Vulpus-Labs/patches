//! E088 / ticket 0496: modules instantiated from a bundle share one
//! `Arc<libloading::Library>`.

use std::path::PathBuf;

use patches_core::modules::{InstanceId, ModuleShape, ParameterMap};
use patches_core::AudioEnvironment;
use patches_registry::ModuleBuilder;
use patches_ffi::loader::load_plugin;

fn dylib_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.push("target");
    path.push("debug");
    #[cfg(target_os = "macos")]
    path.push("libtest_drums_bundle.dylib");
    #[cfg(target_os = "linux")]
    path.push("libtest_drums_bundle.so");
    #[cfg(target_os = "windows")]
    path.push("test_drums_bundle.dll");
    path
}

fn default_env() -> AudioEnvironment {
    AudioEnvironment {
        sample_rate: 48000.0,
        poly_voices: 16,
        periodic_update_interval: 32,
        hosted: false,
    }
}

#[test]
fn two_modules_from_bundle_share_library_handle() {
    let builders = load_plugin(&dylib_path()).expect("load drums bundle");
    assert!(builders.len() >= 8, "expected 8+ builders; got {}", builders.len());

    // Pick the first two distinct builders. Confirm the Arc is shared.
    let arc0 = builders[0].library_arc();
    let arc1 = builders[1].library_arc();
    assert!(std::sync::Arc::ptr_eq(&arc0, &arc1), "builders must share Arc<Library>");
    drop(arc0);
    drop(arc1);

    // Strong count reflects all builders + whatever internal refs.
    let baseline = builders[0].library_strong_count();
    assert!(baseline >= builders.len(), "baseline {baseline} < builder count");

    let env = default_env();
    let shape = ModuleShape::default();
    let params = ParameterMap::new();

    let a = builders[0].build(&env, &shape, &params, InstanceId::next())
        .expect("build 0");
    let b = builders[1].build(&env, &shape, &params, InstanceId::next())
        .expect("build 1");

    // Each DylibModule holds its own Arc clone, so +2 above baseline.
    let with_modules = builders[0].library_strong_count();
    assert!(
        with_modules >= baseline + 2,
        "expected strong_count >= {}, got {}", baseline + 2, with_modules,
    );

    // Drop both modules; strong count must fall back to baseline.
    drop(a);
    drop(b);
    let after = builders[0].library_strong_count();
    assert_eq!(after, baseline, "modules' Arc clones should be released on drop");
}

/// 0562: build two instances from one builder, drop the builder and one
/// instance; the surviving instance's vtable calls must still succeed —
/// the library is only unloaded when the last `Arc<Library>` drops.
#[test]
fn instance_outlives_builder_and_sibling() {
    let mut builders = load_plugin(&dylib_path()).expect("load drums bundle");
    let env = default_env();
    let shape = ModuleShape::default();
    let params = ParameterMap::new();

    let builder = builders.remove(0);
    let a = builder.build(&env, &shape, &params, InstanceId::next()).expect("build a");
    let mut b = builder.build(&env, &shape, &params, InstanceId::next()).expect("build b");

    drop(builder);
    drop(builders);
    drop(a);

    // Vtable call on surviving instance: descriptor access crosses the FFI
    // boundary through the library's symbols. If the library were unloaded
    // this would segfault.
    let desc = b.descriptor().module_name.to_string();
    assert!(!desc.is_empty());
    // Another vtable call: update_parameters round-trips through the plugin.
    let _ = b.update_parameters(&ParameterMap::new());
}
