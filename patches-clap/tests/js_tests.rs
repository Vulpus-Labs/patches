//! Run the vitest suite as part of `cargo test -p patches-clap`.
//! Skipped (with a warning) when npm is unavailable.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn vitest_suite_passes() {
    let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");

    let npm_ok = Command::new("npm")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !npm_ok {
        eprintln!("npm not found; skipping JS test suite");
        return;
    }

    if !assets.join("node_modules").exists() {
        let install = Command::new("npm")
            .args(["install", "--silent", "--no-audit", "--no-fund"])
            .current_dir(&assets)
            .status()
            .expect("npm install spawn");
        assert!(install.success(), "npm install failed: {install}");
    }

    let status = Command::new("npm")
        .args(["test", "--silent"])
        .current_dir(&assets)
        .status()
        .expect("npm test spawn");
    assert!(status.success(), "vitest reported failures (exit {status})");
}
