//! Micro-benchmark for the polyphonic filter hot paths.
//!
//! Measures ns/sample for:
//!   - PolyLowpass (biquad, static-coefficient path, no saturation)
//!   - PolyLowpass (biquad, static-coefficient path, saturation)
//!   - PolySvf     (SVF, static-coefficient path)
//!
//! Run with:
//!   cargo run --example filter_bench --release -p patches-modules

use std::time::Instant;

use patches_core::{
    AudioEnvironment, CablePool, CableValue, InputPort, InstanceId, Module,
    ModuleShape, OutputPort, PolyInput, PolyOutput, POLY_READ_SINK, POLY_WRITE_SINK,
};
use patches_core::parameter_map::ParameterMap;
use patches_modules::{PolyResonantLowpass, PolySvf};

// ── Pool slot constants ────────────────────────────────────────────────────

const POOL_SIZE: usize = 20;

/// Slot holding the audio input signal (filled with a constant test value).
const SIGNAL_SLOT: usize = 16;
/// Slot for the first output.
const OUT1_SLOT: usize = 17;
/// Slot for the second output (used by PolySvf bandpass/highpass).
const OUT2_SLOT: usize = 18;
/// Slot for the third output (used by PolySvf).
const OUT3_SLOT: usize = 19;

// ── Helpers ───────────────────────────────────────────────────────────────

fn make_pool() -> Vec<[CableValue; 2]> {
    let mono_zero = CableValue::Mono(0.0);
    let poly_zero = CableValue::Poly([0.0f32; 16]);
    let signal    = CableValue::Poly([0.3f32; 16]);

    let mut pool = vec![[mono_zero; 2]; POOL_SIZE];
    // Reserved poly slots (MONO_READ_SINK=0 and MONO_WRITE_SINK=2 stay Mono(0.0))
    pool[POLY_READ_SINK]  = [poly_zero; 2]; // 1
    pool[POLY_WRITE_SINK] = [poly_zero; 2]; // 3
    // Signal input: write into both ping-pong slots so both wi=0 and wi=1 see the signal
    pool[SIGNAL_SLOT] = [signal; 2];
    pool[OUT1_SLOT]   = [poly_zero; 2];
    pool[OUT2_SLOT]   = [poly_zero; 2];
    pool[OUT3_SLOT]   = [poly_zero; 2];
    pool
}

fn poly_in(cable_idx: usize, connected: bool) -> InputPort {
    InputPort::Poly(PolyInput { cable_idx, scale: 1.0, connected })
}

fn poly_out(cable_idx: usize, connected: bool) -> OutputPort {
    OutputPort::Poly(PolyOutput { cable_idx, connected })
}

/// Run the module for `n` samples, alternating wi, and return elapsed time.
fn time_process(module: &mut dyn Module, pool: &mut Vec<[CableValue; 2]>, n: u64) -> std::time::Duration {
    let mut wi = 0usize;
    let t0 = Instant::now();
    for _ in 0..n {
        let mut cp = CablePool::new(pool, wi);
        module.process(&mut cp);
        wi = 1 - wi;
    }
    t0.elapsed()
}

// ── PolyLowpass bench ─────────────────────────────────────────────────────

fn bench_poly_lowpass(saturate: bool) {
    let env = AudioEnvironment { sample_rate: 48000.0, poly_voices: 16, periodic_update_interval: 32, hosted: false };
    let mut params = ParameterMap::new();
    params.insert("cutoff".to_string(), patches_core::ParameterValue::Float(6.0));
    params.insert("resonance".to_string(), patches_core::ParameterValue::Float(0.5));
    params.insert("saturate".to_string(), patches_core::ParameterValue::Bool(saturate));

    let mut module = PolyResonantLowpass::build(
        &env,
        &ModuleShape { channels: 0, length: 0, ..Default::default() },
        &params,
        InstanceId::next(),
    ).expect("build failed");

    // Wire: audio input → SIGNAL_SLOT (connected), all CV → disconnected, output → OUT1_SLOT
    let inputs = vec![
        poly_in(SIGNAL_SLOT, true),  // in
        poly_in(POLY_READ_SINK, false), // voct
        poly_in(POLY_READ_SINK, false), // fm
        poly_in(POLY_READ_SINK, false), // resonance_cv
    ];
    let outputs = vec![poly_out(OUT1_SLOT, true)];
    module.set_ports(&inputs, &outputs);

    let mut pool = make_pool();

    // Warmup
    time_process(&mut module, &mut pool, 10_000);

    // Measure
    const N: u64 = 2_000_000;
    let elapsed = time_process(&mut module, &mut pool, N);
    let ns_per_sample = elapsed.as_nanos() as f64 / N as f64;

    let label = if saturate { "PolyLowpass (saturate)" } else { "PolyLowpass (no-sat) " };
    println!("{label}: {:6.1} ns/sample  ({:.3} ms total over {N} samples)", ns_per_sample, elapsed.as_secs_f64() * 1000.0);
}

// ── PolySvf bench ─────────────────────────────────────────────────────────

fn bench_poly_svf() {
    let env = AudioEnvironment { sample_rate: 48000.0, poly_voices: 16, periodic_update_interval: 32, hosted: false };
    let mut params = ParameterMap::new();
    params.insert("cutoff".to_string(), patches_core::ParameterValue::Float(6.0));
    params.insert("q".to_string(), patches_core::ParameterValue::Float(0.5));

    let mut module = PolySvf::build(
        &env,
        &ModuleShape { channels: 0, length: 0, ..Default::default() },
        &params,
        InstanceId::next(),
    ).expect("build failed");

    // Wire: audio input connected, all CV disconnected, all 3 outputs connected
    let inputs = vec![
        poly_in(SIGNAL_SLOT, true),
        poly_in(POLY_READ_SINK, false), // voct
        poly_in(POLY_READ_SINK, false), // fm
        poly_in(POLY_READ_SINK, false), // q_cv
    ];
    let outputs = vec![
        poly_out(OUT1_SLOT, true),  // lowpass
        poly_out(OUT2_SLOT, true),  // highpass
        poly_out(OUT3_SLOT, true),  // bandpass
    ];
    module.set_ports(&inputs, &outputs);

    let mut pool = make_pool();

    // Warmup
    time_process(&mut module, &mut pool, 10_000);

    // Measure
    const N: u64 = 2_000_000;
    let elapsed = time_process(&mut module, &mut pool, N);
    let ns_per_sample = elapsed.as_nanos() as f64 / N as f64;

    println!("PolySvf              : {:6.1} ns/sample  ({:.3} ms total over {N} samples)", ns_per_sample, elapsed.as_secs_f64() * 1000.0);
}

// ── main ──────────────────────────────────────────────────────────────────

fn main() {
    println!("=== filter_bench ===");
    println!();
    bench_poly_lowpass(false);
    bench_poly_lowpass(true);
    bench_poly_svf();
    println!();
    println!("Done.");
}
