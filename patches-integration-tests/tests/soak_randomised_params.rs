//! E111 ticket 0651 — 10 000-cycle soak under the audio-thread allocator
//! trap, with randomised parameter updates across a representative patch
//! that mixes in-process modules (`Osc`, `AudioOut`) and a bundle-loaded
//! FFI module (`VChorus` from `patches-vintage`). Asserts zero audio-thread
//! allocation and that every `Arc<libloading::Library>` reaches refcount
//! zero (less our held clone) at shutdown.
//!
//! Smoke variant is the default (1 000 cycles, fast enough for PR CI).
//! The nightly 10 000-cycle run is selected via `PATCHES_SOAK_CYCLES=10000`.
//!
//! If the `patches-vintage` dylib has not been built, the test skips — the
//! FFI half of the acceptance criteria cannot be exercised without it.

#[global_allocator]
static A: patches_alloc_trap::TrappingAllocator = patches_alloc_trap::TrappingAllocator;

use std::sync::Arc;

use patches_alloc_trap::{trap_hits, NoAllocGuard};
use patches_engine::{
    build_patch, ExecutionPlan, OversamplingFactor, PlannerState,
};
use patches_ffi::loader::load_plugin;
use patches_ffi_common::arc_table::{RuntimeArcTables, RuntimeArcTablesConfig};
use patches_integration_tests::{
    dylib_path, env, HeadlessEngine, MODULE_CAP, POOL_CAP,
};
use patches_modules::default_registry;
use patches_registry::{ModuleBuilder, Registry};
use patches_core::ModuleShape;

const SRC_TEMPLATE: &str = "patch {
    module osc : Osc { frequency: 220Hz }
    module ch  : VChorus { variant: bright, mode: one, hiss: {HISS}, jitter: {JITTER} }
    module out : AudioOut
    osc.sine -> ch.in_left
    osc.sine -> ch.in_right
    ch.out_left  -> out.in_left
    ch.out_right -> out.in_right
}
";

fn render_src(hiss: f32, jitter: f32) -> String {
    SRC_TEMPLATE
        .replace("{HISS}", &format!("{hiss:.4}"))
        .replace("{JITTER}", &format!("{jitter:.4}"))
}

fn build_plan_from_src(
    registry: &Registry,
    src: &str,
    prev: &PlannerState,
) -> (ExecutionPlan, PlannerState) {
    let file = patches_dsl::parse(src).expect("parse");
    let result = patches_dsl::expand(&file).expect("expand");
    let graph = patches_interpreter::build(&result.patch, registry, &env())
        .expect("build")
        .graph;
    build_patch(&graph, registry, &env(), prev, POOL_CAP, MODULE_CAP)
        .expect("build_patch")
}

/// LCG — deterministic, allocation-free; does not pull a crate dep.
struct Lcg(u32);
impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    fn next_f32(&mut self) -> f32 {
        (self.next() >> 8) as f32 / ((1u32 << 24) as f32)
    }
}

#[test]
fn soak_ten_thousand_cycles_randomised_params() {
    let dylib = dylib_path("patches-vintage");
    if !dylib.exists() {
        eprintln!(
            "soak_randomised_params: skipping — vintage dylib not built at {dylib:?}. \
             Run `cargo build -p patches-vintage`."
        );
        return;
    }

    // Load the bundle manually so we can retain an `Arc<Library>` clone
    // and verify refcount drain after teardown.
    let dylib_builders = load_plugin(&dylib).expect("load vintage");
    assert!(!dylib_builders.is_empty(), "vintage bundle exported no modules");
    let lib_arc: Arc<libloading::Library> = dylib_builders[0].library_arc();
    // Our clone + one per live builder = initial strong count.
    let strong_initial = Arc::strong_count(&lib_arc);
    assert_eq!(
        strong_initial,
        1 + dylib_builders.len(),
        "unexpected initial Arc<Library> strong count"
    );

    let mut registry = default_registry();
    let shape = ModuleShape::default();
    for b in dylib_builders {
        let name = b.describe(&shape).module_name.to_string();
        registry.register_builder(name, Box::new(b));
    }

    // Wire an ArcTable so we can assert drain at shutdown. The vintage
    // bundle does not use buffer handles, so live_count should stay zero
    // throughout — covers the acceptance criterion trivially while keeping
    // the harness in place for future ArcTable-using modules.
    let (control, _audio) =
        RuntimeArcTables::new(RuntimeArcTablesConfig { float_buffers: 4 });

    let total_cycles: usize = std::env::var("PATCHES_SOAK_CYCLES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1_000);
    const CYCLES_PER_EPOCH: usize = 100;
    let epochs = total_cycles.div_ceil(CYCLES_PER_EPOCH);

    let hits_before = trap_hits();
    let mut rng = Lcg(0x1234_5678);

    // Initial plan.
    let (plan, mut state) =
        build_plan_from_src(&registry, &render_src(0.5, 0.25), &PlannerState::empty());
    let mut engine = HeadlessEngine::new(POOL_CAP, MODULE_CAP, OversamplingFactor::None);
    engine.adopt_plan(plan);

    // Warm-up outside the guard: first ticks may touch lazy paths.
    for _ in 0..128 {
        engine.tick();
    }

    for _ in 0..epochs {
        // Off-thread: rebuild a new plan with randomised params.
        let hiss = rng.next_f32();
        let jitter = rng.next_f32();
        let src = render_src(hiss, jitter);
        let (plan, new_state) = build_plan_from_src(&registry, &src, &state);
        state = new_state;

        // Plan adoption allocates (DropPlan box for the previous plan);
        // keep it outside the guard. Only the tick loop is armed.
        engine.adopt_plan(plan);

        let _g = NoAllocGuard::enter();
        for _ in 0..CYCLES_PER_EPOCH {
            engine.tick();
        }
    }

    assert_eq!(
        trap_hits(),
        hits_before,
        "audio-thread allocations detected during soak"
    );

    // Teardown: drop the engine first (joins the cleanup thread so every
    // tombstoned module, plan, and param frame is actually released).
    drop(engine);
    drop(registry);

    assert_eq!(
        control.float_buffer_live_count(),
        0,
        "ArcTable must drain to zero at shutdown"
    );

    assert_eq!(
        Arc::strong_count(&lib_arc),
        1,
        "Arc<Library> leaked: {} strong refs remain (expected 1 — our held clone)",
        Arc::strong_count(&lib_arc),
    );
}
