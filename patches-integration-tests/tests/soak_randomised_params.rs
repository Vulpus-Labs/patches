//! E111 ticket 0651 — 10 000-cycle soak under the audio-thread allocator
//! trap, with randomised parameter updates across a representative patch
//! that mixes in-process modules (`Osc`, `AudioOut`) and a bundle-loaded
//! FFI module. Asserts zero audio-thread allocation and that every
//! `Arc<libloading::Library>` reaches refcount zero (less our held clone)
//! at shutdown.
//!
//! Smoke variant is the default (1 000 cycles, fast enough for PR CI).
//! The nightly 10 000-cycle run is selected via `PATCHES_SOAK_CYCLES=10000`.
//!
//! Uses the `test-gain-plugin` cdylib from `test-plugins/gain/` as the
//! bundle subject — the test exercises the FFI loader + audio-thread
//! path, not any particular module's DSP.
//!
//! If the plugin cdylib has not been built, the test skips.

#[global_allocator]
static A: patches_alloc_trap::TrappingAllocator = patches_alloc_trap::TrappingAllocator;

use std::sync::Arc;

use patches_alloc_trap::{trap_hits, NoAllocGuard};
use patches_engine::OversamplingFactor;
use patches_planner::{build_patch, ExecutionPlan, PlannerState};
use patches_ffi::loader::load_plugin;
use patches_integration_tests::{
    dylib_path, env, HeadlessEngine, MODULE_CAP, POOL_CAP,
};
use patches_modules::default_registry;
use patches_core::registry::{ModuleBuilder, Registry};
use patches_core::ModuleShape;

const SRC_TEMPLATE: &str = "patch {
    module osc : Osc { frequency: 220Hz }
    module amp : Gain { gain: {GAIN} }
    module out : AudioOut
    osc.sine -> amp.in
    amp.out -> out.in
}
";

fn render_src(gain: f32) -> String {
    SRC_TEMPLATE.replace("{GAIN}", &format!("{gain:.4}"))
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
    let dylib = dylib_path("test-gain-plugin");
    if !dylib.exists() {
        eprintln!(
            "soak_randomised_params: skipping — test-gain-plugin dylib not built at {dylib:?}. \
             Run `cargo build -p test-gain-plugin`."
        );
        return;
    }

    // Load the plugin manually so we can retain an `Arc<Library>` clone
    // and verify refcount drain after teardown.
    let dylib_builders = load_plugin(&dylib).expect("load gain plugin");
    assert!(!dylib_builders.is_empty(), "test-gain-plugin exported no modules");
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
        let name = b.template().build_channels(shape.channels as u32).module_name.to_string();
        registry.register_builder(name, Box::new(b));
    }

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
        build_plan_from_src(&registry, &render_src(1.0), &PlannerState::empty());
    let mut engine = HeadlessEngine::new(POOL_CAP, MODULE_CAP, OversamplingFactor::None);
    engine.adopt_plan(plan);

    // Warm-up outside the guard: first ticks may touch lazy paths.
    for _ in 0..128 {
        engine.tick();
    }

    for _ in 0..epochs {
        // Off-thread: rebuild a new plan with a randomised gain. The Gain
        // param range is 0..2; bias to the lower half so output stays sane.
        let gain = rng.next_f32() * 2.0;
        let src = render_src(gain);
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
        Arc::strong_count(&lib_arc),
        1,
        "Arc<Library> leaked: {} strong refs remain (expected 1 — our held clone)",
        Arc::strong_count(&lib_arc),
    );
}
