//! Headless timing benchmark for `.patches` files.
//!
//! Usage:  bench [path/to/patch.patches ...]
//!
//! Builds each patch's graph without a CPAL device, warms up for 1 s of audio,
//! then times 10 s of audio sample-by-sample. Reports mean + percentile tick
//! latency and cable-pool memory footprint per patch.

use std::env;
use std::fs;
use std::mem::size_of;
use std::process;
use std::time::Instant;

use patches_core::{
    AudioEnvironment, CablePool, CableValue, CYCLE_CAPACITY, RESERVED_SLOTS, SCRATCH_CAPACITY,
};
use patches_engine::{kernel, ModulePool, ReadyState};
use patches_planner::{build_patch, ExecutionPlan, PlannerState};

const POOL_CAPACITY: usize = 4096;
const MODULE_POOL_CAPACITY: usize = 1024;
const SAMPLE_RATE: f32 = 44_100.0;
const WARMUP_SECONDS: f64 = 1.0;
const BENCH_SECONDS: f64 = 10.0;

fn load_plan(path: &str) -> ExecutionPlan {
    let registry = patches_modules::default_registry();
    let load = patches_dsl::load_with(std::path::Path::new(path), |p| fs::read_to_string(p))
        .unwrap_or_else(|e| {
            eprintln!("{path}: load error: {e}");
            process::exit(1);
        });
    let file = load.file;
    let result = patches_dsl::expand(&file).unwrap_or_else(|e| {
        eprintln!("{path}: expand error: {e}");
        process::exit(1);
    });
    for w in &result.warnings {
        eprintln!("{path}: dsl warning: {w}");
    }
    let env = AudioEnvironment {
        sample_rate: SAMPLE_RATE,
        poly_voices: 16,
        periodic_update_interval: 32,
        hosted: false,
    };
    let build_result =
        patches_interpreter::build(&result.patch, &registry, &env).unwrap_or_else(|e| {
            eprintln!("{path}: interpreter error: {e}");
            process::exit(1);
        });
    let graph = build_result.graph;
    let (plan, _) = build_patch(
        &graph,
        &registry,
        &env,
        &PlannerState::empty(),
        POOL_CAPACITY,
        MODULE_POOL_CAPACITY,
    )
    .unwrap_or_else(|e| {
        eprintln!("{path}: build error: {e}");
        process::exit(1);
    });
    plan
}

fn run_ticks(
    state: &mut ReadyState,
    cycle_pool: &mut [[CableValue; 2]],
    scratch_pool: &mut [CableValue],
    n: u64,
    wi_in: usize,
) -> usize {
    let mut wi = wi_in;
    for _ in 0..n {
        let mut cp = CablePool::new(scratch_pool, cycle_pool, wi);
        state.tick(&mut cp);
        wi = 1 - wi;
    }
    wi
}

fn time_ticks(
    state: &mut ReadyState,
    cycle_pool: &mut [[CableValue; 2]],
    scratch_pool: &mut [CableValue],
    n: u64,
    wi_in: usize,
) -> (Vec<u64>, usize) {
    let mut samples = Vec::with_capacity(n as usize);
    let mut wi = wi_in;
    for _ in 0..n {
        let t0 = Instant::now();
        let mut cp = CablePool::new(scratch_pool, cycle_pool, wi);
        state.tick(&mut cp);
        let dt = t0.elapsed().as_nanos() as u64;
        samples.push(dt);
        wi = 1 - wi;
    }
    (samples, wi)
}

fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx]
}

fn bench_patch(path: &str) {
    println!("─── {path} ─────────────────────────────────────────────");

    let mut plan = load_plan(path);
    let cycle_hwm = plan.cycle_hwm;
    let scratch_hwm = plan.scratch_hwm;

    let mut cycle_pool: Box<[[CableValue; 2]]> = kernel::init_cycle_pool();
    let mut scratch_pool: Box<[CableValue]> = kernel::init_scratch_pool(POOL_CAPACITY);

    let mut module_pool = ModulePool::new(MODULE_POOL_CAPACITY);
    let module_state_iter = plan.new_module_param_state.drain(..);
    for ((idx, m), ps) in plan.new_modules.drain(..).zip(module_state_iter) {
        module_pool.install(idx, m, ps);
    }
    // Apply zero lists that the engine would normally apply on plan adoption.
    for &i in &plan.to_zero {
        if i < SCRATCH_CAPACITY {
            scratch_pool[i] = CableValue::mono(0.0);
        } else {
            cycle_pool[i - SCRATCH_CAPACITY] = [CableValue::mono(0.0), CableValue::mono(0.0)];
        }
    }
    for &i in &plan.to_zero_poly {
        if i < SCRATCH_CAPACITY {
            scratch_pool[i] = CableValue::poly([0.0; 16]);
        } else {
            cycle_pool[i - SCRATCH_CAPACITY] = [CableValue::poly([0.0; 16]), CableValue::poly([0.0; 16])];
        }
    }

    let stale = ReadyState::new_stale(module_pool);
    let mut state = stale.rebuild(&plan, patches_core::BASE_PERIODIC_UPDATE_INTERVAL);

    let warmup_samples = (WARMUP_SECONDS * SAMPLE_RATE as f64) as u64;
    let bench_samples = (BENCH_SECONDS * SAMPLE_RATE as f64) as u64;

    let wi = run_ticks(&mut state, &mut cycle_pool, &mut scratch_pool, warmup_samples, 0);

    let (samples, _) = time_ticks(&mut state, &mut cycle_pool, &mut scratch_pool, bench_samples, wi);

    let mut sorted = samples.clone();
    sorted.sort_unstable();
    let total: u64 = samples.iter().sum();
    let mean = total as f64 / samples.len() as f64;
    let p50 = percentile(&sorted, 0.50);
    let p99 = percentile(&sorted, 0.99);
    let p999 = percentile(&sorted, 0.999);
    let max = *sorted.last().unwrap_or(&0);

    let budget_ns = 1_000_000_000.0 / SAMPLE_RATE as f64;
    let headroom = budget_ns / mean;

    let cycle_alloc_bytes = cycle_pool.len() * size_of::<[CableValue; 2]>();
    let scratch_alloc_bytes = scratch_pool.len() * size_of::<CableValue>();
    let total_alloc = cycle_alloc_bytes + scratch_alloc_bytes;
    let cycle_used_bytes = cycle_hwm * size_of::<[CableValue; 2]>();
    let scratch_used_bytes = scratch_hwm * size_of::<CableValue>();
    let used_total = cycle_used_bytes + scratch_used_bytes;

    println!("  ticks:       {bench_samples}");
    println!("  mean:        {mean:>8.1} ns/tick");
    println!("  p50:         {p50:>8} ns");
    println!("  p99:         {p99:>8} ns");
    println!("  p99.9:       {p999:>8} ns");
    println!("  max:         {max:>8} ns");
    println!("  budget:      {budget_ns:>8.1} ns/tick (@{SAMPLE_RATE} Hz)");
    println!("  headroom:    {headroom:>8.2}× (mean)");
    println!(
        "  cycle:       {cycle_hwm:>4} / {} slots used = {cycle_used_bytes} / {cycle_alloc_bytes} bytes",
        cycle_pool.len()
    );
    println!(
        "  scratch:     {scratch_hwm:>4} / {} slots used = {scratch_used_bytes} / {scratch_alloc_bytes} bytes",
        scratch_pool.len()
    );
    println!("  pool used:   {used_total} bytes");
    println!("  pool alloc:  {total_alloc} bytes");
    println!("  fas_size:    {}", plan.fas_size);
    println!();
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let paths: Vec<String> = if args.is_empty() {
        vec!["examples/poly_synth_layered.patches".to_owned()]
    } else {
        args
    };

    println!(
        "phase-5 bench (SCRATCH_CAPACITY={SCRATCH_CAPACITY}, CYCLE_CAPACITY={CYCLE_CAPACITY}, RESERVED_SLOTS={RESERVED_SLOTS}, POOL_CAPACITY={POOL_CAPACITY})"
    );
    println!();

    for path in &paths {
        bench_patch(path);
    }
}
