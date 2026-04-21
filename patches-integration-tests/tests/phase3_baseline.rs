//! Phase 3 baseline WAV snapshot. Throwaway — delete after E101 Phase 3 lands.
//!
//! Renders each candidate patch to raw stereo f32 LE bytes under
//! /tmp/phase3-baseline/<name>.f32. Run `shasum -a 256` on each to produce
//! the parity digest set. After trait flip, re-run and diff digests.

use patches_integration_tests::{build_engine, env, HeadlessEngine};
use patches_modules::default_registry;
use std::fs::File;
use std::io::Write;

const SAMPLES: usize = 44_100 * 4; // 4 seconds @ 44.1kHz

fn build_from(path_rel: &str) -> HeadlessEngine {
    let root = format!("{}/..", env!("CARGO_MANIFEST_DIR"));
    let path = format!("{}/{}", root, path_rel);
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {}", path, e));
    let file = patches_dsl::parse(&src).expect("parse");
    let result = patches_dsl::expand(&file).expect("expand");
    let registry = default_registry();
    let graph = patches_interpreter::build(&result.patch, &registry, &env())
        .expect("build")
        .graph;
    build_engine(&graph, &registry)
}

fn render(path_rel: &str, out_name: &str) {
    let mut engine = build_from(path_rel);
    let mut bytes: Vec<u8> = Vec::with_capacity(SAMPLES * 8);
    // Warmup
    for _ in 0..128 { engine.tick(); }
    let mut nonzero = 0usize;
    for _ in 0..SAMPLES {
        engine.tick();
        let l = engine.last_left();
        let r = engine.last_right();
        if l != 0.0 || r != 0.0 { nonzero += 1; }
        bytes.extend_from_slice(&l.to_le_bytes());
        bytes.extend_from_slice(&r.to_le_bytes());
    }
    let out = format!("/tmp/phase3-baseline/{}.f32", out_name);
    let mut f = File::create(&out).expect("open out");
    f.write_all(&bytes).expect("write");
    eprintln!("WROTE {} ({} nonzero / {} samples)", out, nonzero, SAMPLES);
}

#[test]
fn baseline_drum_machine() { render("examples/drum_machine.patches", "drum_machine"); }

#[test]
fn baseline_tracker_three_voices() { render("examples/tracker_three_voices.patches", "tracker_three_voices"); }

#[test]
fn baseline_pentatonic_sah() { render("examples/pentatonic_sah.patches", "pentatonic_sah"); }

#[test]
fn baseline_radigue_drone() { render("examples/radigue_drone.patches", "radigue_drone"); }

#[test]
fn baseline_square_440() { render("examples/square_440.patches", "square_440"); }

#[test]
fn baseline_poly_noise_synth() { render("examples/poly_noise_synth.patches", "poly_noise_synth"); }

#[test]
fn baseline_pad() { render("examples/pad.patches", "pad"); }

#[test]
fn baseline_soft_pad() { render("examples/soft_pad.patches", "soft_pad"); }

#[test]
fn baseline_fdn_reverb_synth() { render("examples/fdn_reverb_synth.patches", "fdn_reverb_synth"); }

#[test]
fn baseline_fm_synth() { render("examples/fm_synth.patches", "fm_synth"); }

#[test]
fn baseline_poly_synth() { render("examples/poly_synth.patches", "poly_synth"); }
