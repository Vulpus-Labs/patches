//! Tickets 0628 (committed baseline) + 0629 (bundle-loaded parity oracle).
//!
//! Renders `fixtures/vintage_baseline.patches` through the DSL pipeline
//! against a registry built from `patches_modules::default_registry()` plus
//! the `patches-vintage` cdylib scanned at runtime (ADR 0045 Spike 8
//! Phase E). Asserts the stereo f32 LE byte stream matches the committed
//! golden file and its SHA-256 digest.
//!
//! The golden file was generated during Phase A while vintage modules still
//! lived in the in-process `default_registry()`. Bit-identical equality
//! after Phase C removed them proves the bundle-loaded data plane is
//! indistinguishable from the old in-process path.
//!
//! If the vintage dylib has not been built (e.g. `cargo test` in a minimal
//! workspace build without the cdylib artifact), the parity test is
//! skipped, not failed — matching the convention documented in ticket 0571.
//!
//! # Regenerating the golden artifacts
//!
//! First ensure the vintage bundle exists:
//!
//!     cargo build -p patches-vintage
//!     cargo test -p patches-integration-tests --test vintage_baseline \
//!         -- --ignored regenerate_vintage_baseline

use patches_ffi::scanner::PluginScanner;
use patches_integration_tests::{build_engine, dylib_path, env, HeadlessEngine};
use patches_modules::default_registry;
use patches_registry::Registry;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

// Fixed rendering parameters. Any change invalidates the golden file.
const SR: f32 = 44_100.0;
const SAMPLES: usize = 8192;
const WARMUP: usize = 0;

const FIXTURE_REL: &str = "fixtures/vintage_baseline.patches";
const GOLDEN_REL:  &str = "fixtures/vintage_baseline.f32";
const HASH_REL:    &str = "fixtures/vintage_baseline.sha256";

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Build a registry from `default_registry()` and scan the vintage dylib on
/// top. Returns `None` if the dylib is missing (tests downgrade to "skipped").
fn registry_with_vintage_bundle() -> Option<Registry> {
    let dylib = dylib_path("patches-vintage");
    if !dylib.exists() {
        eprintln!(
            "vintage_baseline: skipping — dylib not built at {dylib:?}. \
             Run `cargo build -p patches-vintage` first."
        );
        return None;
    }
    let mut registry = default_registry();
    let report = PluginScanner::new([dylib]).scan(&mut registry);
    assert!(
        report.errors.is_empty(),
        "vintage bundle scan reported errors: {:?}",
        report.errors
    );
    assert!(
        !report.loaded.is_empty(),
        "vintage bundle scan loaded nothing (skipped: {:?})",
        report.skipped
    );
    Some(registry)
}

fn build_from_fixture(registry: &Registry) -> HeadlessEngine {
    let src_path = crate_dir().join(FIXTURE_REL);
    let src = std::fs::read_to_string(&src_path)
        .unwrap_or_else(|e| panic!("read {src_path:?}: {e}"));
    assert_eq!(env().sample_rate, SR, "env() sample rate changed");
    let file = patches_dsl::parse(&src).expect("parse");
    let result = patches_dsl::expand(&file).expect("expand");
    let graph = patches_interpreter::build(&result.patch, registry, &env())
        .expect("build")
        .graph;
    build_engine(&graph, registry)
}

fn render_stereo_bytes(registry: &Registry) -> Vec<u8> {
    let mut engine = build_from_fixture(registry);
    for _ in 0..WARMUP { engine.tick(); }
    let mut bytes = Vec::with_capacity(SAMPLES * 8);
    for _ in 0..SAMPLES {
        engine.tick();
        bytes.extend_from_slice(&engine.last_left().to_le_bytes());
        bytes.extend_from_slice(&engine.last_right().to_le_bytes());
    }
    bytes
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let out = hasher.finalize();
    let mut s = String::with_capacity(out.len() * 2);
    for b in out.iter() {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

#[test]
#[ignore]
fn regenerate_vintage_baseline() {
    let registry = registry_with_vintage_bundle()
        .expect("regenerate requires a built vintage dylib; run `cargo build -p patches-vintage`");
    let bytes = render_stereo_bytes(&registry);
    let hex = sha256_hex(&bytes);

    let golden = crate_dir().join(GOLDEN_REL);
    if let Some(p) = golden.parent() { std::fs::create_dir_all(p).expect("mkdir"); }
    std::fs::write(&golden, &bytes)
        .unwrap_or_else(|e| panic!("write {golden:?}: {e}"));

    let hash_path = crate_dir().join(HASH_REL);
    std::fs::write(&hash_path, format!("{hex}\n"))
        .unwrap_or_else(|e| panic!("write {hash_path:?}: {e}"));

    eprintln!(
        "Wrote {} bytes ({} stereo f32 samples) and sha256 {hex}",
        bytes.len(),
        SAMPLES,
    );
}

#[test]
fn vintage_baseline_matches_golden() {
    let Some(registry) = registry_with_vintage_bundle() else {
        return; // Dylib missing: skip per ticket 0571 convention.
    };
    let golden_path = crate_dir().join(GOLDEN_REL);
    let hash_path   = crate_dir().join(HASH_REL);

    let golden = std::fs::read(&golden_path)
        .unwrap_or_else(|e| panic!(
            "golden file missing at {golden_path:?}: {e}\n\
             run `cargo test -p patches-integration-tests --test vintage_baseline \
             -- --ignored regenerate_vintage_baseline` to create it"
        ));
    let expected_hex = std::fs::read_to_string(&hash_path)
        .unwrap_or_else(|e| panic!("hash file missing at {hash_path:?}: {e}"))
        .trim()
        .to_owned();

    let actual = render_stereo_bytes(&registry);
    let actual_hex = sha256_hex(&actual);

    assert_eq!(
        actual.len(), golden.len(),
        "sample count mismatch: got {}, golden {}",
        actual.len(), golden.len(),
    );
    assert_eq!(
        actual_hex, expected_hex,
        "sha256 mismatch: got {actual_hex}, expected {expected_hex}",
    );
    assert_eq!(actual, golden, "byte-for-byte mismatch vs golden");
}
