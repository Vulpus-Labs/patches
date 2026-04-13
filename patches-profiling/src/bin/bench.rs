//! Headless timing benchmark for a `.patches` file.
//!
//! Usage:  bench [path/to/patch.patches]
//!
//! Builds the patch graph without a CPAL device, warms up for 1 s of audio,
//! then times 10 s of audio in a tight loop and prints ns/sample and the
//! real-time ratio (× headroom; higher is better).

use std::env;
use std::fs;
use std::process;
use std::time::Instant;

use patches_core::{
    AudioEnvironment, CablePool, CableValue, PlannerState, POLY_READ_SINK, POLY_WRITE_SINK,
};
use patches_engine::{build_patch, ExecutionPlan, ReadyState, ModulePool};

const POOL_CAPACITY: usize = 4096;
const MODULE_POOL_CAPACITY: usize = 1024;
const SAMPLE_RATE: f32 = 44_100.0;

fn load_plan(path: &str) -> ExecutionPlan {
    let registry = patches_modules::default_registry();
    let src = fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("error reading {path}: {e}");
        process::exit(1);
    });
    let file = patches_dsl::parse(&src).unwrap_or_else(|e| {
        eprintln!("parse error: {e}");
        process::exit(1);
    });
    let result = patches_dsl::expand(&file).unwrap_or_else(|e| {
        eprintln!("expand error: {e}");
        process::exit(1);
    });
    for w in &result.warnings {
        eprintln!("dsl warning: {w}");
    }
    let env = AudioEnvironment { sample_rate: SAMPLE_RATE, poly_voices: 16, periodic_update_interval: 32, hosted: false };
    let build_result = patches_interpreter::build(&result.patch, &registry, &env).unwrap_or_else(|e| {
        eprintln!("interpreter error: {e}");
        process::exit(1);
    });
    let graph = build_result.graph;
    let (plan, _) =
        build_patch(&graph, &registry, &env, &PlannerState::empty(), POOL_CAPACITY, MODULE_POOL_CAPACITY)
            .unwrap_or_else(|e| {
                eprintln!("build error: {e}");
                process::exit(1);
            });
    plan
}

fn run_ticks(
    state: &mut ReadyState,
    buffer_pool: &mut Box<[[CableValue; 2]]>,
    n: u64,
) {
    let mut wi = 0usize;
    for _ in 0..n {
        let mut cable_pool = CablePool::new(buffer_pool, wi);
        // SAFETY: state was rebuilt from a consistent plan + pool before this call.
        state.tick(&mut cable_pool);
        wi = 1 - wi;
    }
}

fn main() {
    let path = env::args()
        .nth(1)
        .unwrap_or_else(|| "examples/poly_synth_layered.patches".to_owned());

    println!("patch:   {path}");
    let mut plan = load_plan(&path);

    // Initialise buffer pool, mirroring SoundEngine / HeadlessEngine setup.
    let mut buffer_pool: Box<[[CableValue; 2]]> = (0..POOL_CAPACITY)
        .map(|_| [CableValue::Mono(0.0), CableValue::Mono(0.0)])
        .collect::<Vec<_>>()
        .into_boxed_slice();
    buffer_pool[POLY_READ_SINK] = [CableValue::Poly([0.0; 16]), CableValue::Poly([0.0; 16])];
    buffer_pool[POLY_WRITE_SINK] = [CableValue::Poly([0.0; 16]), CableValue::Poly([0.0; 16])];

    let mut module_pool = ModulePool::new(MODULE_POOL_CAPACITY);
    for (idx, m) in plan.new_modules.drain(..) {
        module_pool.install(idx, m);
    }
    let stale = ReadyState::new_stale(module_pool);
    let mut state = stale.rebuild(&plan, patches_core::BASE_PERIODIC_UPDATE_INTERVAL);

    let warmup_samples = SAMPLE_RATE as u64;
    let bench_samples = (10.0 * SAMPLE_RATE) as u64;

    print!("warming up ({warmup_samples} samples)… ");
    run_ticks(&mut state, &mut buffer_pool, warmup_samples);
    println!("done");

    print!("benchmarking ({bench_samples} samples)… ");
    let t0 = Instant::now();
    run_ticks(&mut state, &mut buffer_pool, bench_samples);
    let elapsed = t0.elapsed();
    println!("done");

    let ns_per_sample = elapsed.as_nanos() as f64 / bench_samples as f64;
    let budget_ns = 1_000_000_000.0 / SAMPLE_RATE as f64;
    let rt_headroom = budget_ns / ns_per_sample;

    println!();
    println!("  elapsed:    {:.3} s", elapsed.as_secs_f64());
    println!("  ns/sample:  {ns_per_sample:.2}");
    println!("  budget:     {budget_ns:.2} ns/sample  (@{SAMPLE_RATE} Hz)");
    println!("  headroom:   {rt_headroom:.2}×  (higher is better)");
}
