//! Audio-thread allocator trap.
//!
//! Installs a global allocator that aborts on any `alloc` / `dealloc` /
//! `realloc` call made while a per-thread "no alloc" flag is set. The test
//! wraps the audio-thread entry point (`HeadlessEngine::tick`) inside a
//! guard that raises the flag. Any allocation — from the engine, the module
//! pool, module `process()` methods, or anything downstream — aborts the
//! process with `SIGTRAP`-visible call stack.
//!
//! If this test blows up, the stack trace shows which allocation slipped
//! into the audio path. If it passes, the in-process audio loop is clean
//! for the graphs it exercises.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use patches_integration_tests::{build_engine, env, run_n_stereo};
use patches_modules::default_registry;

// ── Trapping allocator ────────────────────────────────────────────────────────

struct TrappingAllocator;

thread_local! {
    static NO_ALLOC_ACTIVE: Cell<bool> = const { Cell::new(false) };
}

/// Set to true the first time the guard is entered, so allocator hooks know
/// the mechanism is live. Before the first entry we let everything through
/// unconditionally — avoids tripping on static initialisers.
static TRAP_ARMED: AtomicBool = AtomicBool::new(false);

/// Number of times the trap would have fired. We record and abort so that
/// the debugger catches the exact stack; the counter is mostly here for
/// any future "count but don't abort" soft mode.
static TRAP_HITS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for TrappingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if TRAP_ARMED.load(Ordering::Relaxed) {
            NO_ALLOC_ACTIVE.with(|f| {
                if f.get() {
                    TRAP_HITS.fetch_add(1, Ordering::Relaxed);
                    // abort, not panic — panic! itself allocates.
                    std::process::abort();
                }
            });
        }
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if TRAP_ARMED.load(Ordering::Relaxed) {
            NO_ALLOC_ACTIVE.with(|f| {
                if f.get() {
                    TRAP_HITS.fetch_add(1, Ordering::Relaxed);
                    std::process::abort();
                }
            });
        }
        System.dealloc(ptr, layout)
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if TRAP_ARMED.load(Ordering::Relaxed) {
            NO_ALLOC_ACTIVE.with(|f| {
                if f.get() {
                    TRAP_HITS.fetch_add(1, Ordering::Relaxed);
                    std::process::abort();
                }
            });
        }
        System.alloc_zeroed(layout)
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if TRAP_ARMED.load(Ordering::Relaxed) {
            NO_ALLOC_ACTIVE.with(|f| {
                if f.get() {
                    TRAP_HITS.fetch_add(1, Ordering::Relaxed);
                    std::process::abort();
                }
            });
        }
        System.realloc(ptr, layout, new_size)
    }
}

#[global_allocator]
static A: TrappingAllocator = TrappingAllocator;

// ── Guard ─────────────────────────────────────────────────────────────────────

struct NoAllocGuard;

impl NoAllocGuard {
    fn enter() -> Self {
        TRAP_ARMED.store(true, Ordering::Relaxed);
        NO_ALLOC_ACTIVE.with(|f| f.set(true));
        NoAllocGuard
    }
}

impl Drop for NoAllocGuard {
    fn drop(&mut self) {
        NO_ALLOC_ACTIVE.with(|f| f.set(false));
    }
}

// ── Test ──────────────────────────────────────────────────────────────────────

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

/// Drive the engine for many ticks with the alloc trap armed around the
/// hot path. Any allocation inside `tick()` aborts the test process.
#[test]
fn audio_tick_performs_no_allocation() {
    let mut engine = build_simple_engine();

    // Warm-up: first few ticks may allocate in paths that haven't been
    // exercised yet (e.g. lazy backplane slot initialisation). Run them
    // outside the guard.
    for _ in 0..16 {
        engine.tick();
    }

    // Armed loop. 4096 ticks ≈ 93 ms of audio at 44.1 kHz.
    let guard = NoAllocGuard::enter();
    for _ in 0..4096 {
        engine.tick();
    }
    drop(guard);

    // Prevent the engine from dropping inside any accidental residual
    // guard scope. (Drop allocates to join the cleanup thread etc.)
    drop(engine);

    assert_eq!(TRAP_HITS.load(Ordering::Relaxed), 0);
}

/// Same mechanism, broader graph: run a realistic stereo signal chain.
#[test]
fn audio_tick_no_allocation_stereo_batch() {
    let mut engine = build_simple_engine();
    // Warm-up.
    let _ = run_n_stereo(&mut engine, 64);

    let guard = NoAllocGuard::enter();
    for _ in 0..8192 {
        engine.tick();
    }
    drop(guard);

    drop(engine);
    assert_eq!(TRAP_HITS.load(Ordering::Relaxed), 0);
}

fn sweep(path_rel: &str, warmup: usize, iters: usize) {
    let mut engine = build_engine_from(path_rel);
    for _ in 0..warmup {
        engine.tick();
    }
    let guard = NoAllocGuard::enter();
    for _ in 0..iters {
        engine.tick();
    }
    drop(guard);
    drop(engine);
    assert_eq!(TRAP_HITS.load(Ordering::Relaxed), 0);
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
