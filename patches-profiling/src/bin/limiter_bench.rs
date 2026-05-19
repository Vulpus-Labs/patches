//! Microbenchmark for Limiter / StereoLimiter ::process.
//!
//! Runs N ticks against a [`ModuleHarness`] feeding a sinusoidal signal that
//! exceeds the limiter threshold (so the gain path is active). Prints mean and
//! percentile per-tick latency. Pass `mono` as first arg to bench the mono
//! variant; default is stereo.

use std::env;
use std::hint::black_box;
use std::time::Instant;

use patches_core::test_support::ModuleHarness;
use patches_core::{AudioEnvironment, ModuleShape};
use patches_modules::{Limiter, StereoLimiter};

const SR: f32 = 48_000.0;
const ENV: AudioEnvironment = AudioEnvironment {
    sample_rate: SR,
    poly_voices: 16,
    periodic_update_interval: 32,
    hosted: false,
};

fn percentile(sorted: &[u64], p: f64) -> u64 {
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx]
}

fn bench_stereo(n: usize, warmup: usize) -> (Vec<u64>, std::time::Duration) {
    let mut h = ModuleHarness::build_full::<StereoLimiter>(
        &[],
        ENV,
        ModuleShape { channels: 0 },
    );
    for i in 0..warmup {
        let t = i as f32 / SR;
        let s = 2.0 * (std::f32::consts::TAU * 1_000.0 * t).sin();
        h.set_stereo("in", s, 0.7 * s);
        h.tick();
    }
    const BATCH: usize = 256;
    let batches = n / BATCH;
    let mut batch_ns: Vec<u64> = Vec::with_capacity(batches);
    let t_total = Instant::now();
    let mut idx = warmup;
    for _ in 0..batches {
        let t0 = Instant::now();
        for _ in 0..BATCH {
            let t = idx as f32 / SR;
            let s = 2.0 * (std::f32::consts::TAU * 1_000.0 * t).sin();
            h.set_stereo("in", s, 0.7 * s);
            h.tick();
            black_box(h.read_stereo("out"));
            idx += 1;
        }
        batch_ns.push(t0.elapsed().as_nanos() as u64);
    }
    let elapsed = t_total.elapsed();
    let per = batch_ns.iter().map(|&b| b / BATCH as u64).collect();
    (per, elapsed)
}

fn bench_mono(n: usize, warmup: usize) -> (Vec<u64>, std::time::Duration) {
    let mut h = ModuleHarness::build_full::<Limiter>(
        &[],
        ENV,
        ModuleShape { channels: 0 },
    );
    for i in 0..warmup {
        let t = i as f32 / SR;
        let s = 2.0 * (std::f32::consts::TAU * 1_000.0 * t).sin();
        h.set_mono("in", s);
        h.tick();
    }
    const BATCH: usize = 256;
    let batches = n / BATCH;
    let mut batch_ns: Vec<u64> = Vec::with_capacity(batches);
    let t_total = Instant::now();
    let mut idx = warmup;
    for _ in 0..batches {
        let t0 = Instant::now();
        for _ in 0..BATCH {
            let t = idx as f32 / SR;
            let s = 2.0 * (std::f32::consts::TAU * 1_000.0 * t).sin();
            h.set_mono("in", s);
            h.tick();
            black_box(h.read_mono("out"));
            idx += 1;
        }
        batch_ns.push(t0.elapsed().as_nanos() as u64);
    }
    let elapsed = t_total.elapsed();
    let per = batch_ns.iter().map(|&b| b / BATCH as u64).collect();
    (per, elapsed)
}

fn main() {
    let mut args = env::args().skip(1);
    let first = args.next();
    let mode = first.as_deref().unwrap_or("stereo");
    let n: usize = match mode {
        "mono" | "stereo" => args.next().and_then(|s| s.parse().ok()).unwrap_or(2_000_000),
        _ => first.as_ref().and_then(|s| s.parse().ok()).unwrap_or(2_000_000),
    };
    let mode = if mode == "mono" { "mono" } else { "stereo" };
    let warmup: usize = (SR as usize) / 2;

    let (samples, elapsed_total) = if mode == "mono" {
        bench_mono(n, warmup)
    } else {
        bench_stereo(n, warmup)
    };
    let mut samples = samples;

    samples.sort_unstable();
    let sum: u128 = samples.iter().map(|&x| x as u128).sum();
    let mean = sum as f64 / samples.len() as f64;
    let p50 = percentile(&samples, 0.50);
    let p90 = percentile(&samples, 0.90);
    let p99 = percentile(&samples, 0.99);
    let p999 = percentile(&samples, 0.999);
    let max = *samples.last().unwrap();
    let min = *samples.first().unwrap();

    let realtime_budget_ns_per_sample = 1e9 / SR as f64;
    let cpu_pct = (mean / realtime_budget_ns_per_sample) * 100.0;

    let label = if mode == "mono" { "Limiter" } else { "StereoLimiter" };
    println!("{label}::tick — n={n} samples, sr={SR} Hz");
    println!("  total wall: {:.2} ms", elapsed_total.as_secs_f64() * 1e3);
    println!(
        "  per-tick ns: min={min}  mean={mean:.1}  p50={p50}  p90={p90}  p99={p99}  p99.9={p999}  max={max}"
    );
    println!(
        "  realtime cost: {cpu_pct:.2}% of 1-core (budget {realtime_budget_ns_per_sample:.1} ns/sample @ {SR} Hz)"
    );
}
