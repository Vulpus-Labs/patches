//! E107 ticket 0622 — descriptor-hash mismatch refuses load.

use patches_ffi::loader::load_plugin;
use patches_integration_tests::dylib_path;

#[test]
fn bad_hash_plugin_refuses_to_load() {
    let path = dylib_path("test-gain-bad-hash-plugin");
    let err = match load_plugin(&path) {
        Err(e) => e,
        Ok(_) => panic!("bad-hash plugin must fail to load"),
    };
    assert!(err.contains("descriptor_hash mismatch"), "error was: {err}");
    assert!(err.contains("Gain"), "error should name module: {err}");
    assert!(err.contains("0xdeadbeef"), "error should show plugin hash: {err}");
}
