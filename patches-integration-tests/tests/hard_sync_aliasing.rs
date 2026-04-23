//! Ticket 0637 / ADR 0047: hard-sync aliasing comparison.
//!
//! Two chains drive a slave `Osc`'s sawtooth via hard-sync from a master `Osc`:
//!
//! * **Direct** — `master.reset_out` → `slave.sync` (typed sub-sample sync).
//! * **Via pulse** — `master.reset_out` → `SyncToTrigger` → `TriggerToSync` →
//!   `slave.sync`. The converter round-trip discards the fractional position,
//!   simulating sample-boundary rounding of the sync event.
//!
//! For each sync ratio we render the slave sawtooth and take an FFT via
//! `patches_dsp::RealPackedFft`,
//! then sum the magnitude in the upper quarter of the spectrum (`3·N/8 ..
//! N/2`, i.e. above 18 kHz at 48 kHz). PolyBLEP-smoothed hard-sync output
//! concentrates most of its energy in the first few harmonics; what lands
//! in the top band is dominated by aliasing artefacts. Sample-boundary
//! rounding of the sync event (the "via-pulse" chain) injects broadband
//! high-frequency noise on top of the PolyBLEP residual, lifting the top
//! band. We assert the via-pulse chain's high-band energy exceeds the
//! direct chain's by at least `ALIAS_MARGIN`.
//!
//! # Methodology notes
//!
//! * `SR = 48000`, `N = 4096` → 11.72 Hz bin width. `f_master = 468.75 Hz`
//!   (bin-aligned at k=40); chosen so the master's own harmonic lines fall
//!   on bin centres under a rectangular window, which avoids confounding
//!   the measurement with spectral leakage from the intended signal.
//! * `WARMUP` ticks discard transients from phase initialisation and the
//!   converter chain's one-sample-per-module delay.
//! * The margin is tuned once against the committed converter / oscillator
//!   behaviour. If it regresses the test will fail noisily, pointing at
//!   whatever change broke the sub-sample accuracy guarantee.

use patches_core::parameter_map::{ParameterMap, ParameterValue};
use patches_core::{AudioEnvironment, Module, ModuleGraph, ModuleShape, NodeId};
use patches_engine::{build_patch, OversamplingFactor, PlannerState};
use patches_integration_tests::{p, HeadlessEngine, MODULE_CAP, POOL_CAP};
use patches_modules::{AudioOut, Oscillator, SyncToTrigger, TriggerToSync};

const SR: f32 = 48_000.0;
const N: usize = 4096;
const WARMUP: usize = 4096;
const C0_FREQ: f32 = 16.3516;
/// `40 * SR / N = 468.75 Hz` — bin-aligned so master harmonics land on bin
/// centres under a rectangular window.
const F_MASTER: f32 = 468.75;
/// Minimum ratio by which the via-pulse high-band energy must exceed the
/// direct chain's. Tuned against the committed trigger/sync pipeline: most
/// ratios observe 1.6×, the 2:1 case ~1.3× (slave phase lands near zero at
/// every sync event so frac discarding has the least effect there). 1.2×
/// captures the floor with modest headroom — regressions that narrow the
/// gap will fail, pointing at a loss of sub-sample accuracy on the typed
/// path.
const ALIAS_MARGIN: f64 = 1.2;

fn voct(freq_hz: f32) -> f32 {
    (freq_hz / C0_FREQ).log2()
}

fn env_sr() -> AudioEnvironment {
    AudioEnvironment { sample_rate: SR, poly_voices: 16, periodic_update_interval: 32, hosted: false }
}

fn shape() -> ModuleShape {
    ModuleShape { channels: 0, length: 0, ..Default::default() }
}

fn params_freq(v: f32) -> ParameterMap {
    let mut pm = ParameterMap::new();
    pm.insert("frequency".to_string(), ParameterValue::Float(v));
    pm
}

fn build_direct(f_master: f32, f_slave: f32) -> ModuleGraph {
    let mut g = ModuleGraph::new();
    g.add_module("master", Oscillator::describe(&shape()), &params_freq(voct(f_master))).unwrap();
    g.add_module("slave",  Oscillator::describe(&shape()), &params_freq(voct(f_slave))).unwrap();
    g.add_module("out",    AudioOut::describe(&shape()), &ParameterMap::new()).unwrap();
    g.connect(&NodeId::from("master"), p("reset_out"), &NodeId::from("slave"), p("sync"), 1.0).unwrap();
    g.connect(&NodeId::from("slave"), p("sawtooth"), &NodeId::from("out"), p("in_left"), 1.0).unwrap();
    g.connect(&NodeId::from("slave"), p("sawtooth"), &NodeId::from("out"), p("in_right"), 1.0).unwrap();
    g
}

fn build_via_pulse(f_master: f32, f_slave: f32) -> ModuleGraph {
    let mut g = ModuleGraph::new();
    g.add_module("master", Oscillator::describe(&shape()), &params_freq(voct(f_master))).unwrap();
    g.add_module("slave",  Oscillator::describe(&shape()), &params_freq(voct(f_slave))).unwrap();
    g.add_module("s2t",    SyncToTrigger::describe(&shape()), &ParameterMap::new()).unwrap();
    g.add_module("t2s",    TriggerToSync::describe(&shape()), &ParameterMap::new()).unwrap();
    g.add_module("out",    AudioOut::describe(&shape()), &ParameterMap::new()).unwrap();
    g.connect(&NodeId::from("master"), p("reset_out"), &NodeId::from("s2t"), p("in"), 1.0).unwrap();
    g.connect(&NodeId::from("s2t"), p("out"), &NodeId::from("t2s"), p("in"), 1.0).unwrap();
    g.connect(&NodeId::from("t2s"), p("out"), &NodeId::from("slave"), p("sync"), 1.0).unwrap();
    g.connect(&NodeId::from("slave"), p("sawtooth"), &NodeId::from("out"), p("in_left"), 1.0).unwrap();
    g.connect(&NodeId::from("slave"), p("sawtooth"), &NodeId::from("out"), p("in_right"), 1.0).unwrap();
    g
}

fn render(graph: &ModuleGraph) -> Vec<f32> {
    let registry = patches_modules::default_registry();
    let env = env_sr();
    let (plan, _) =
        build_patch(graph, &registry, &env, &PlannerState::empty(), POOL_CAP, MODULE_CAP)
            .expect("build_patch");
    let mut eng = HeadlessEngine::new(POOL_CAP, MODULE_CAP, OversamplingFactor::None);
    eng.adopt_plan(plan);
    for _ in 0..WARMUP { eng.tick(); }
    let mut samples = Vec::with_capacity(N);
    for _ in 0..N {
        eng.tick();
        samples.push(eng.last_left());
    }
    samples
}

/// Magnitude spectrum (bins `0..N/2`, DC through the bin below Nyquist),
/// rectangular window. The analysed signal is periodic in the window length
/// by construction, so no window is needed.
fn spectrum_magnitudes(x: &[f32]) -> Vec<f64> {
    let n = x.len();
    let fft = patches_dsp::RealPackedFft::new(n);
    let mut buf: Vec<f32> = x.to_vec();
    fft.forward(&mut buf);
    // Packed layout: [0]=DC.re, [1]=Nyquist.re, [2k]=X[k].re, [2k+1]=X[k].im.
    let mut mags = vec![0.0f64; n / 2];
    mags[0] = (buf[0] as f64).abs();
    for k in 1..n / 2 {
        let re = buf[2 * k] as f64;
        let im = buf[2 * k + 1] as f64;
        mags[k] = (re * re + im * im).sqrt();
    }
    mags
}

/// Sum of magnitudes in the upper half of the spectrum (bins `N/4..N/2`,
/// i.e. frequencies above `SR/4 = 12 kHz`). A hard-synced sawtooth smoothed
/// by PolyBLEP has most of its energy in the first ~10 harmonics; what lands
/// in the upper band is dominated by aliasing. Sample-boundary rounding of
/// the sync event injects broadband noise, lifting this band. Using a simple
/// band sum (rather than picking out individual harmonic bins) keeps the
/// measurement robust to the exact pattern period, which depends on the
/// master's sub-sample wrap cadence and differs per ratio.
fn high_band_energy(mags: &[f64]) -> f64 {
    let half = mags.len();
    let start = (half * 3) / 4;
    mags[start..].iter().copied().sum()
}

fn compare_at_ratio(ratio: f32, label: &str) {
    let f_master: f32 = F_MASTER;
    let f_slave = f_master * ratio;

    let a = render(&build_direct(f_master, f_slave));
    let b = render(&build_via_pulse(f_master, f_slave));

    assert!(a.iter().all(|v| v.is_finite()), "{label}: direct produced non-finite samples");
    assert!(b.iter().all(|v| v.is_finite()), "{label}: via-pulse produced non-finite samples");

    let mags_a = spectrum_magnitudes(&a);
    let mags_b = spectrum_magnitudes(&b);
    let alias_a = high_band_energy(&mags_a);
    let alias_b = high_band_energy(&mags_b);
    let _ = f_master;

    assert!(
        alias_b > alias_a * ALIAS_MARGIN,
        "{label} (ratio={ratio}): expected via-pulse aliasing ({alias_b:.4}) \
         to exceed direct ({alias_a:.4}) by >{ALIAS_MARGIN}×"
    );
}

#[test]
fn hard_sync_direct_beats_via_pulse_3_to_2() { compare_at_ratio(1.5, "3:2"); }

#[test]
fn hard_sync_direct_beats_via_pulse_2_to_1() { compare_at_ratio(2.0, "2:1"); }

#[test]
fn hard_sync_direct_beats_via_pulse_golden() { compare_at_ratio(1.618_034, "golden"); }

#[test]
fn hard_sync_direct_beats_via_pulse_7_to_2() { compare_at_ratio(3.5, "7:2"); }

// ── DSL fixtures ─────────────────────────────────────────────────────────────

/// Smoke: the committed `.patches` fixtures parse, expand, and build against
/// the default registry. They document the two topologies the aliasing tests
/// above exercise programmatically.
#[test]
fn dsl_fixtures_build() {
    for name in ["hard_sync_direct.patches", "hard_sync_via_pulse.patches"] {
        let path = format!(
            "{}/../patches-dsl/tests/fixtures/{}",
            env!("CARGO_MANIFEST_DIR"),
            name
        );
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {path}: {e}"));
        let file = patches_dsl::parse(&src).expect("parse");
        let result = patches_dsl::expand(&file).expect("expand");
        let registry = patches_modules::default_registry();
        let env = env_sr();
        patches_interpreter::build(&result.patch, &registry, &env).expect("build");
    }
}
