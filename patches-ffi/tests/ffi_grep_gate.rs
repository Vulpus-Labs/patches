//! Cargo-test entry point for the FFI audio-path grep gate.
//!
//! Runs `tools/ffi-grep-gate.sh` against `patches-ffi/src/loader.rs` from the
//! workspace root. A violation fails the build (ticket 0627, epic E108).

use std::path::PathBuf;
use std::process::Command;

#[test]
fn ffi_audio_path_grep_gate() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.parent().expect("workspace root");
    let script = workspace_root.join("tools").join("ffi-grep-gate.sh");
    let target = workspace_root.join("patches-ffi").join("src").join("loader.rs");

    assert!(script.exists(), "script missing: {}", script.display());
    assert!(target.exists(), "target missing: {}", target.display());

    let output = Command::new("bash")
        .arg(&script)
        .arg(&target)
        .output()
        .expect("failed to spawn grep-gate");

    if !output.status.success() {
        panic!(
            "ffi-grep-gate failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
}
