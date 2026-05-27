//! Micro-benchmark + ASM-gate harness for `HyperSawCore` (ticket 0958).
//!
//! Measures ns/sample for the full working size (9 copies × 16 voices = 144
//! saws) and reports headroom vs the 44.1 kHz budget. The hot loop in
//! `HyperSawCore::process` must autovectorise (ADR 0078 §7); inspect the
//! generated assembly alongside this number, e.g.
//!
//!   # aarch64 / NEON (local):
//!   cargo asm -p patches-dsp --example hypersaw_bench --release \
//!       'patches_dsp::hypersaw::HyperSawCore::process'
//!
//!   # x86-64 / AVX2 (CI/release target):
//!   RUSTFLAGS="-C target-feature=+avx2" cargo asm ... # same symbol
//!
//! Confirm vector ops (NEON `fmla`/`v*.4s`, or AVX2 `vfmadd*ps`/`ymm`) in the
//! per-sample body. A scalar hot loop fails the gate (ADR 0078 §7 fallback).
//!
//! Run:
//!   cargo run --example hypersaw_bench --release -p patches-dsp

use std::time::Instant;

use patches_dsp::hypersaw::{HyperSawCore, N_COPIES, N_VOICES};

fn main() {
    let sr = 44_100.0f32;
    let base = 110.0f32;

    // Build a realistic full-stack update: 16 voices, 9 detuned copies each,
    // spread over ±50 cents, loudness-normalised.
    let to_inc = |f: f32| (f / sr * 4_294_967_296.0) as u32;
    let mut inc = [[0u32; N_VOICES]; N_COPIES];
    let mut inv = [[0.0f32; N_VOICES]; N_COPIES];
    let mut gain = [[0.0f32; N_VOICES]; N_COPIES];
    let g = 1.0 / N_COPIES as f32;
    for k in 0..N_COPIES {
        let cents = (k as f32 - 4.0) * 12.5;
        for v in 0..N_VOICES {
            let f = base * 2.0f32.powf(v as f32 / 12.0) * 2.0f32.powf(cents / 1200.0);
            let i = to_inc(f);
            inc[k][v] = i;
            inv[k][v] = 1.0 / i as f32;
            gain[k][v] = g;
        }
    }

    let mut core = HyperSawCore::new(0x1234_5678_9abc_def0);
    core.update(&inc, &inv, &gain);

    let mut out = [0.0f32; N_VOICES];
    let mut sink = 0.0f32;

    // Warm up.
    for _ in 0..10_000 {
        core.process(&mut out);
        sink += out[0];
    }

    let iters = 2_000_000usize;
    let start = Instant::now();
    for _ in 0..iters {
        core.process(&mut out);
        sink += out[0]; // defeat dead-code elimination
    }
    let elapsed = start.elapsed();

    let ns_per_sample = elapsed.as_nanos() as f64 / iters as f64;
    // One audio sample must be produced every 1/sr seconds across all voices.
    let budget_ns = 1.0e9 / sr as f64;
    let headroom = budget_ns / ns_per_sample;

    println!("HyperSawCore: 9 copies × 16 voices = 144 saws/sample");
    println!("  {ns_per_sample:.2} ns/sample ({:.3} ns/saw)", ns_per_sample / 144.0);
    println!("  44.1 kHz budget: {budget_ns:.1} ns/sample → {headroom:.0}× headroom");
    println!("  (sink={sink})");
}
