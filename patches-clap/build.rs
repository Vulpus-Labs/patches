//! Build script: regenerate the webview JS bundle from `assets/src/`
//! using esbuild when sources change. The bundle (`assets/app.bundle.js`)
//! is committed so `cargo build` works without Node installed; this
//! script only updates it when JS sources actually change *and* npm is
//! available.
//!
//! JS tests run via the integration test in `tests/js_tests.rs` so
//! `cargo test -p patches-clap` exercises them.

use std::path::Path;
use std::process::Command;

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR set");
    let assets = Path::new(&manifest_dir).join("assets");

    // Only re-run the build script when JS sources, the build script
    // itself, or the package manifest change. The bundle output is not
    // listed — listing it would create a rerun loop.
    println!("cargo:rerun-if-changed=assets/src");
    println!("cargo:rerun-if-changed=assets/build/bundle.mjs");
    println!("cargo:rerun-if-changed=assets/package.json");
    println!("cargo:rerun-if-changed=assets/package-lock.json");
    println!("cargo:rerun-if-changed=build.rs");

    if !which("npm") {
        println!("cargo:warning=npm not found; skipping JS bundle rebuild — using committed assets/app.bundle.js");
        return;
    }

    // Install deps only if node_modules is missing. Subsequent runs
    // skip this and go straight to the bundle step.
    if !assets.join("node_modules").exists() {
        run_npm(&assets, &["install", "--silent", "--no-audit", "--no-fund"]);
    }

    run_npm(&assets, &["run", "build"]);
}

fn which(cmd: &str) -> bool {
    Command::new(cmd)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_npm(cwd: &Path, args: &[&str]) {
    let status = Command::new("npm")
        .args(args)
        .current_dir(cwd)
        .status()
        .expect("failed to spawn npm");
    if !status.success() {
        panic!("npm {args:?} failed with {status}");
    }
}
