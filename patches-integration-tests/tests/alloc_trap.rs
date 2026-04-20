//! Audio-thread allocator trap (ADR 0045 spike 4).
//!
//! Uses the shared `patches-alloc-trap` crate's `TrappingAllocator` as the
//! global allocator. Each test wraps the hot path in a `NoAllocGuard` that
//! tags the current thread as audio-thread for the duration of the scope;
//! any allocation inside that scope aborts the process (or, under
//! `TrapMode::Count`, increments a counter for test assertion).
//!
//! When the `audio-thread-allocator-trap` feature is off, the allocator
//! compiles to a transparent `System` forward and all guards are no-ops —
//! the tests still run and pass, but no trapping occurs.

#[global_allocator]
static A: patches_alloc_trap::TrappingAllocator = patches_alloc_trap::TrappingAllocator;

use patches_alloc_trap::{trap_hits, NoAllocGuard};
use patches_integration_tests::{build_engine, env, run_n_stereo};
use patches_modules::default_registry;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn load(path_rel: &str) -> String {
    let path = format!("{}/../{}", env!("CARGO_MANIFEST_DIR"), path_rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read '{}': {}", path, e))
}

fn build_engine_from(path_rel: &str) -> patches_integration_tests::HeadlessEngine {
    let src = load(path_rel);
    let file = patches_dsl::parse(&src).expect("parse failed");
    let result = patches_dsl::expand(&file).expect("expand failed");
    let registry = default_registry();
    let graph = patches_interpreter::build(&result.patch, &registry, &env())
        .expect("build failed")
        .graph;
    build_engine(&graph, &registry)
}

fn build_simple_engine() -> patches_integration_tests::HeadlessEngine {
    build_engine_from("patches-dsl/tests/fixtures/simple.patches")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Drive the engine for many ticks with the alloc trap armed around the
/// hot path. Any allocation inside `tick()` aborts the test process.
#[test]
fn audio_tick_performs_no_allocation() {
    let hits_before = trap_hits();
    let mut engine = build_simple_engine();

    // Warm-up: first few ticks may allocate in paths that haven't been
    // exercised yet (e.g. lazy backplane slot initialisation). Run them
    // outside the guard.
    for _ in 0..16 {
        engine.tick();
    }

    // Armed loop. 4096 ticks ≈ 93 ms of audio at 44.1 kHz.
    {
        let _g = NoAllocGuard::enter();
        for _ in 0..4096 {
            engine.tick();
        }
    }

    // Prevent the engine from dropping inside any accidental residual
    // guard scope. (Drop allocates to join the cleanup thread etc.)
    drop(engine);

    assert_eq!(trap_hits(), hits_before);
}

/// Same mechanism, broader graph: run a realistic stereo signal chain.
#[test]
fn audio_tick_no_allocation_stereo_batch() {
    let hits_before = trap_hits();
    let mut engine = build_simple_engine();
    // Warm-up.
    let _ = run_n_stereo(&mut engine, 64);

    {
        let _g = NoAllocGuard::enter();
        for _ in 0..8192 {
            engine.tick();
        }
    }

    drop(engine);
    assert_eq!(trap_hits(), hits_before);
}

fn sweep(path_rel: &str, warmup: usize, iters: usize) {
    let hits_before = trap_hits();
    let mut engine = build_engine_from(path_rel);
    for _ in 0..warmup {
        engine.tick();
    }
    {
        let _g = NoAllocGuard::enter();
        for _ in 0..iters {
            engine.tick();
        }
    }
    drop(engine);
    assert_eq!(trap_hits(), hits_before);
}

#[test]
fn audio_tick_no_alloc_poly_synth() {
    sweep("examples/poly_synth.patches", 128, 4096);
}

#[test]
fn audio_tick_no_alloc_fm_synth() {
    sweep("examples/fm_synth.patches", 128, 4096);
}

#[test]
fn audio_tick_no_alloc_fdn_reverb_synth() {
    sweep("examples/fdn_reverb_synth.patches", 128, 4096);
}

#[test]
fn audio_tick_no_alloc_pad() {
    sweep("examples/pad.patches", 128, 4096);
}

#[test]
fn audio_tick_no_alloc_pentatonic_sah() {
    sweep("examples/pentatonic_sah.patches", 128, 4096);
}

#[test]
fn audio_tick_no_alloc_drum_machine() {
    sweep("examples/drum_machine.patches", 128, 4096);
}

#[test]
fn audio_tick_no_alloc_tracker_three_voices() {
    sweep("examples/tracker_three_voices.patches", 128, 4096);
}
