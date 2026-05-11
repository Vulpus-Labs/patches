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
    ModuleShape, OutputPort, PolyInput, PolyOutput, POLY_READ_SINK,
    RESERVED_SLOTS, SCRATCH_CAPACITY,
};
use patches_core::parameter_map::ParameterMap;
use patches_modules::{PolyResonantLowpass, PolySvf};

// ── Pool slot constants ────────────────────────────────────────────────────
//
// Bench uses cycle slots for the signal and outputs so we exercise the
// ping-pong read path (matches the engine's behaviour for cables with
// at least one delayed consumer). Absolute `cable_idx` is
// `SCRATCH_CAPACITY + logical`. Disconnected CV inputs route to
// `POLY_READ_SINK` (a reserved scratch slot) via `PolyInput::default()`, which
// flags `fused: true`.

const CYCLE_POOL_SIZE: usize = 4;

const SIGNAL_LOGICAL: usize = 0;
const OUT1_LOGICAL: usize = 1;
const OUT2_LOGICAL: usize = 2;
const OUT3_LOGICAL: usize = 3;

const fn cidx(i: usize) -> usize { SCRATCH_CAPACITY + i }

// ── Helpers ───────────────────────────────────────────────────────────────

fn make_pool() -> (Vec<CableValue>, Vec<[CableValue; 2]>) {
    let poly_zero = CableValue::poly([0.0f32; 16]);
    let signal    = CableValue::poly([0.3f32; 16]);

    let mut scratch = vec![CableValue::mono(0.0); RESERVED_SLOTS];
    scratch[POLY_READ_SINK] = poly_zero;

    let mut cycle = vec![[poly_zero; 2]; CYCLE_POOL_SIZE];
    cycle[SIGNAL_LOGICAL] = [signal; 2];
    (scratch, cycle)
}

fn poly_in(cable_idx: usize, connected: bool) -> InputPort {
    if connected {
        InputPort::Poly(PolyInput {
            cable_idx, scale: 1.0, offset: 0.0, clip: None, connected: true, fused: false,
        })
    } else {
        InputPort::Poly(PolyInput::default())
    }
}

fn poly_out(cable_idx: usize, connected: bool) -> OutputPort {
    OutputPort::Poly(PolyOutput { cable_idx, connected })
}

/// Run the module for `n` samples, alternating wi, and return elapsed time.
fn time_process(
    module: &mut dyn Module,
    scratch: &mut [CableValue],
    cycle: &mut [[CableValue; 2]],
    n: u64,
) -> std::time::Duration {
    let mut wi = 0usize;
    let t0 = Instant::now();
    for _ in 0..n {
        let mut cp = CablePool::new(scratch, cycle, wi);
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
        &ModuleShape { channels: 0 },
        &params,
        &patches_core::StructuralParams::new(),
        InstanceId::next(),
    ).expect("build failed");

    // Wire: audio input → SIGNAL cycle slot, all CV → disconnected, output → OUT1 cycle slot.
    let inputs = vec![
        poly_in(cidx(SIGNAL_LOGICAL), true),  // in
        poly_in(POLY_READ_SINK, false), // voct
        poly_in(POLY_READ_SINK, false), // fm
        poly_in(POLY_READ_SINK, false), // resonance_cv
    ];
    let outputs = vec![poly_out(cidx(OUT1_LOGICAL), true)];
    module.set_ports(&inputs, &outputs);

    let (mut scratch, mut cycle) = make_pool();

    // Warmup
    time_process(&mut module, &mut scratch, &mut cycle, 10_000);

    // Measure
    const N: u64 = 2_000_000;
    let elapsed = time_process(&mut module, &mut scratch, &mut cycle, N);
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
        &ModuleShape { channels: 0 },
        &params,
        &patches_core::StructuralParams::new(),
        InstanceId::next(),
    ).expect("build failed");

    // Wire: audio input connected, all CV disconnected, all 3 outputs connected
    let inputs = vec![
        poly_in(cidx(SIGNAL_LOGICAL), true),
        poly_in(POLY_READ_SINK, false), // voct
        poly_in(POLY_READ_SINK, false), // fm
        poly_in(POLY_READ_SINK, false), // q_cv
    ];
    let outputs = vec![
        poly_out(cidx(OUT1_LOGICAL), true),  // lowpass
        poly_out(cidx(OUT2_LOGICAL), true),  // highpass
        poly_out(cidx(OUT3_LOGICAL), true),  // bandpass
    ];
    module.set_ports(&inputs, &outputs);

    let (mut scratch, mut cycle) = make_pool();

    // Warmup
    time_process(&mut module, &mut scratch, &mut cycle, 10_000);

    // Measure
    const N: u64 = 2_000_000;
    let elapsed = time_process(&mut module, &mut scratch, &mut cycle, N);
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
