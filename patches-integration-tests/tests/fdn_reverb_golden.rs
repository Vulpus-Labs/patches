//! T-0218 — golden-file / reference comparison
//!
//! Compares FDN reverb impulse response against the stored golden file.
//! Reference: generated at 48000 Hz with fixed parameters (see golden/README.md).
//! Tolerance: max absolute difference < 1e-4 per sample.
//!
//! # Regenerating the golden file
//!
//! Run with the `--ignored` flag to regenerate:
//!
//!     cargo test -p patches-integration-tests -- --ignored generate_fdn_reverb_golden_file

use patches_core::{AudioEnvironment, test_support::{ModuleHarness, params}};
use patches_modules::FdnReverb;

/// Parameters and setup used for both generation and verification.
const SR: f32 = 48_000.0;
const N_SAMPLES: usize = 2048;

/// Path to the golden file, relative to the workspace root.
const GOLDEN_PATH: &str = "patches-integration-tests/golden/fdn_reverb_impulse.bin";

fn workspace_root() -> std::path::PathBuf {
    // CARGO_MANIFEST_DIR is set to the crate root (patches-integration-tests/).
    // Walk one level up to reach the workspace root.
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR not set");
    std::path::PathBuf::from(manifest_dir)
        .parent()
        .expect("patches-integration-tests has no parent directory")
        .to_owned()
}

fn golden_path() -> std::path::PathBuf {
    workspace_root().join(GOLDEN_PATH)
}

/// Build a `ModuleHarness` for FdnReverb at the golden parameters.
///
/// - character: "plate"  (short delays, fast response)
/// - size: 0.5, brightness: 0.5
/// - modulation: 0.0 (LFO depth is fixed in the module; this keeps the
///   output deterministic by starting LFO phases at zero and using the
///   plate archetype's minimal LFO depth).
/// - stereo output connected (out_left + out_right)
/// - size_cv and brightness_cv disconnected
fn make_harness() -> ModuleHarness {
    let env = AudioEnvironment {
        sample_rate: SR,
        poly_voices: 16,
        periodic_update_interval: 32,
        hosted: false,
    };
    let mut h = ModuleHarness::build_with_env::<FdnReverb>(
        params!["size" => 0.5_f32, "brightness" => 0.5_f32, "character" => patches_modules::fdn_reverb::params::Character::Plate],
        env,
    );
    // Disconnect CV inputs and the right audio input (mono source).
    h.disconnect_input("in_right");
    h.disconnect_input("size_cv");
    h.disconnect_input("brightness_cv");
    h.disconnect_input("pre_delay_cv");
    h.disconnect_input("mix_cv");
    // Keep both outputs connected so stereo_out = true and we capture L+R.
    h
}

/// Run the harness with a unit impulse and collect [left0, right0, left1, right1, ...].
fn run_impulse_response(h: &mut ModuleHarness) -> Vec<f32> {
    let mut samples = Vec::with_capacity(N_SAMPLES * 2);

    // Sample 0: impulse
    h.set_mono("in_left", 1.0);
    h.tick();
    samples.push(h.read_mono("out_left"));
    samples.push(h.read_mono("out_right"));

    // Samples 1..N_SAMPLES: silence
    h.set_mono("in_left", 0.0);
    for _ in 1..N_SAMPLES {
        h.tick();
        samples.push(h.read_mono("out_left"));
        samples.push(h.read_mono("out_right"));
    }

    samples
}

/// Generate and write the golden file. Run with `-- --ignored` to regenerate.
#[test]
#[ignore]
fn generate_fdn_reverb_golden_file() {
    let mut h = make_harness();
    let samples = run_impulse_response(&mut h);

    let path = golden_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .expect("failed to create golden directory");
    }

    let bytes: Vec<u8> = samples
        .iter()
        .flat_map(|&v| v.to_le_bytes())
        .collect();

    std::fs::write(&path, &bytes)
        .unwrap_or_else(|e| panic!("failed to write golden file {path:?}: {e}"));

    println!(
        "Wrote {} f32 samples ({} bytes) to {path:?}",
        samples.len(),
        bytes.len()
    );
}

/// Compare the current FDN reverb impulse response against the golden file.
///
/// Tolerance: max absolute difference < 1e-4 per sample.
#[test]
fn fdn_reverb_matches_golden_file() {
    let path = golden_path();
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|e| panic!(
            "golden file not found at {path:?}: {e}\n\
             Run `cargo test -p patches-integration-tests -- --ignored generate_fdn_reverb_golden_file` \
             to generate it."
        ));

    let expected_byte_count = N_SAMPLES * 2 * 4; // 2 channels * 4 bytes/f32
    assert_eq!(
        bytes.len(),
        expected_byte_count,
        "golden file has unexpected size: {} bytes (expected {})",
        bytes.len(),
        expected_byte_count
    );

    let golden: Vec<f32> = bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect();

    let mut h = make_harness();
    let actual = run_impulse_response(&mut h);

    assert_eq!(actual.len(), golden.len(), "sample count mismatch");

    let max_diff = actual
        .iter()
        .zip(golden.iter())
        .map(|(a, g)| (a - g).abs())
        .fold(0.0_f32, f32::max);

    assert!(
        max_diff < 1e-4,
        "FDN reverb output differs from golden file: max_abs_diff = {max_diff:.3e} \
         (tolerance = 1e-4)"
    );
}
