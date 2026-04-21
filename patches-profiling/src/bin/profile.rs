//! Per-module timing breakdown for a `.patches` file.
//!
//! Usage:  profile [path/to/patch.patches]
//!
//! Wraps every module in a `TimingShim` after a warm-up pass, runs the full
//! signal chain through a plain tick loop, and collects wall-clock ns/call
//! inside the shims. Results are printed in two sections: a per-type summary
//! and a per-instance detail table, both sorted by total time descending.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::process;
use std::sync::Arc;

use patches_core::{
    AudioEnvironment, CablePool, CableValue, Module,
    POLY_READ_SINK, POLY_WRITE_SINK,
};
use patches_engine::{ReadyState, ModulePool};
use patches_planner::{build_patch, PlannerState};
use patches_profiling::timing_collector::TimingCollector;
use patches_profiling::timing_shim::TimingShim;

const POOL_CAPACITY: usize = 4096;
const MODULE_POOL_CAPACITY: usize = 1024;
/// Hardware output sample rate (used for budget / headroom calculation).
const DEVICE_RATE: f32 = 44_100.0;
/// Inner-tick multiplier (1 = no oversampling, 2 = 2× oversampling, …).
const OVERSAMPLING_FACTOR: usize = 4;
/// Sample rate seen by modules (device rate × oversampling factor).
const SAMPLE_RATE: f32 = DEVICE_RATE * OVERSAMPLING_FACTOR as f32;
/// Output frames to run during the profiling pass.
const PROFILE_ITERS: u64 = 200_000;
/// Warm-up ticks to run before timing begins (in inner ticks).
const WARMUP_TICKS: u64 = 44_100;

fn main() {
    let path = env::args()
        .nth(1)
        .unwrap_or_else(|| "examples/poly_synth_layered.patches".to_owned());

    println!("patch:  {path}");

    let registry = patches_modules::default_registry();
    let load = patches_dsl::load_with(std::path::Path::new(&path), |p| fs::read_to_string(p)).unwrap_or_else(|e| {
        eprintln!("load error: {e}");
        process::exit(1);
    });
    let file = load.file;
    let result = patches_dsl::expand(&file).unwrap_or_else(|e| {
        eprintln!("expand error: {e}");
        process::exit(1);
    });
    for w in &result.warnings {
        eprintln!("dsl warning: {w}");
    }
    let env = AudioEnvironment {
        sample_rate: SAMPLE_RATE,
        poly_voices: 16,
        periodic_update_interval: patches_core::BASE_PERIODIC_UPDATE_INTERVAL * OVERSAMPLING_FACTOR as u32,
        hosted: false,
    };
    let build_result = patches_interpreter::build(&result.patch, &registry, &env).unwrap_or_else(|e| {
        eprintln!("interpreter error: {e}");
        process::exit(1);
    });
    let graph = build_result.graph;
    let (mut plan, _state) = build_patch(
        &graph,
        &registry,
        &env,
        &PlannerState::empty(),
        POOL_CAPACITY,
        MODULE_POOL_CAPACITY,
    )
    .unwrap_or_else(|e| {
        eprintln!("build error: {e}");
        process::exit(1);
    });

    // Drain new_modules + parallel param state; keep pool indices for later re-wrap.
    let raw_modules: Vec<(usize, Box<dyn Module>)> = plan.new_modules.drain(..).collect();
    let mut raw_param_state: Vec<patches_planner::ParamState> =
        plan.new_module_param_state.drain(..).collect();
    let pool_indices: Vec<usize> = raw_modules.iter().map(|&(idx, _)| idx).collect();

    let mut buffer_pool: Box<[[CableValue; 2]]> = (0..POOL_CAPACITY)
        .map(|_| [CableValue::Mono(0.0), CableValue::Mono(0.0)])
        .collect::<Vec<_>>()
        .into_boxed_slice();
    buffer_pool[POLY_READ_SINK] = [CableValue::Poly([0.0; 16]), CableValue::Poly([0.0; 16])];
    buffer_pool[POLY_WRITE_SINK] = [CableValue::Poly([0.0; 16]), CableValue::Poly([0.0; 16])];
    // Apply the plan's cable zeroing lists (mirrors the receive_plan sequence in callback.rs).
    for &i in &plan.to_zero {
        buffer_pool[i] = [CableValue::Mono(0.0), CableValue::Mono(0.0)];
    }
    for &i in &plan.to_zero_poly {
        buffer_pool[i] = [CableValue::Poly([0.0; 16]), CableValue::Poly([0.0; 16])];
    }

    let mut module_pool = ModulePool::new(MODULE_POOL_CAPACITY);
    for ((idx, m), ps) in raw_modules.into_iter().zip(raw_param_state.drain(..)) {
        module_pool.install(idx, m, ps);
    }
    let stale = ReadyState::new_stale(module_pool);
    let mut state = stale.rebuild(&plan, env.periodic_update_interval);

    // Warm up with raw modules so module state is representative before timing.
    print!("warming up ({WARMUP_TICKS} inner ticks, {OVERSAMPLING_FACTOR}× oversampling)… ");
    let mut wi = 0usize;
    for _ in 0..WARMUP_TICKS {
        let mut cp = CablePool::new(&mut buffer_pool, wi);
        // SAFETY: state was rebuilt above; no tombstoning since then.
        state.tick(&mut cp);
        wi = 1 - wi;
    }
    println!("done");

    // Tombstone raw modules, wrap each in a TimingShim, and re-install.
    let collector = Arc::new(TimingCollector::new());
    let mut stale = state.make_stale();
    {
        let pool = stale.module_pool_mut();
        for &idx in &pool_indices {
            let (raw, param_state) = pool.tombstone(idx);
            if let (Some(raw), Some(ps)) = (raw, param_state) {
                let shim = Box::new(TimingShim::new(raw, Arc::clone(&collector)));
                pool.install(idx, shim, ps);
            }
        }
    }
    // Rebuild state after swapping raw modules for shims.
    let mut state = stale.rebuild(&plan, env.periodic_update_interval);

    println!(
        "profiling {} slots × {PROFILE_ITERS} output frames × {OVERSAMPLING_FACTOR}× oversampling…",
        plan.slots.len()
    );
    println!();

    // Profile pass — all measurement happens inside the shims.
    // For OVERSAMPLING_FACTOR > 1 we run `factor` inner ticks per output frame,
    // matching the AudioCallback inner loop.  The shims accumulate ns across all
    // inner ticks; headroom is calculated per output frame (not per inner tick).
    for _ in 0..PROFILE_ITERS {
        for _ in 0..OVERSAMPLING_FACTOR {
            let mut cp = CablePool::new(&mut buffer_pool, wi);
            // SAFETY: state was rebuilt above after shim re-installation.
            state.tick(&mut cp);
            wi = 1 - wi;
        }
    }

    // ── Report ────────────────────────────────────────────────────────────────
    let records = collector.report();

    // Per-type aggregation.
    struct TypeStats {
        count: u32,
        proc_calls: u64,
        proc_ns: u64,
        per_calls: u64,
        per_ns: u64,
    }
    let mut by_name: HashMap<&'static str, TypeStats> = HashMap::new();
    for r in &records {
        let e = by_name.entry(r.module_name).or_insert(TypeStats {
            count: 0,
            proc_calls: 0,
            proc_ns: 0,
            per_calls: 0,
            per_ns: 0,
        });
        e.count += 1;
        e.proc_calls += r.process_calls;
        e.proc_ns += r.process_total_ns;
        e.per_calls += r.periodic_calls;
        e.per_ns += r.periodic_total_ns;
    }

    let total_ns: u64 = by_name.values().map(|e| e.proc_ns + e.per_ns).sum();

    let mut type_rows: Vec<(&'static str, TypeStats)> = by_name.into_iter().collect();
    type_rows.sort_by(|a, b| (b.1.proc_ns + b.1.per_ns).cmp(&(a.1.proc_ns + a.1.per_ns)));

    println!("── Per-type summary ──────────────────────────────────────────────────");
    println!(
        "{:<24} {:>5}  {:>12}  {:>12}  {:>8}",
        "module", "inst", "proc ns/call", "per ns/call", "%"
    );
    println!("{}", "─".repeat(68));
    for (name, s) in &type_rows {
        let avg_proc = if s.proc_calls > 0 { s.proc_ns as f64 / s.proc_calls as f64 } else { 0.0 };
        let per_str = if s.per_calls > 0 {
            format!("{:>12.2}", s.per_ns as f64 / s.per_calls as f64)
        } else {
            format!("{:>12}", "—")
        };
        let combined = s.proc_ns + s.per_ns;
        let pct = if total_ns > 0 { combined as f64 / total_ns as f64 * 100.0 } else { 0.0 };
        println!("{:<24} {:>5}  {:>12.2}  {}  {:>7.1}%", name, s.count, avg_proc, per_str, pct);
    }
    println!("{}", "─".repeat(68));
    println!(
        "{:<24} {:>5}  {:>12}  {:>12}  {:>7.1}%",
        "TOTAL", records.len(), "", "", 100.0_f64,
    );

    println!();
    println!("── Per-instance detail ───────────────────────────────────────────────");
    println!(
        "{:<24} {:>20}  {:>12}  {:>12}  {:>8}",
        "module", "instance_id", "proc ns/call", "per ns/call", "%"
    );
    println!("{}", "─".repeat(82));
    for r in &records {
        let avg_proc = if r.process_calls > 0 {
            r.process_total_ns as f64 / r.process_calls as f64
        } else {
            0.0
        };
        let per_str = if r.periodic_calls > 0 {
            format!("{:>12.2}", r.periodic_total_ns as f64 / r.periodic_calls as f64)
        } else {
            format!("{:>12}", "—")
        };
        let pct = if total_ns > 0 { r.total_ns() as f64 / total_ns as f64 * 100.0 } else { 0.0 };
        println!(
            "{:<24} {:>20}  {:>12.2}  {}  {:>7.1}%",
            r.module_name, r.instance_id, avg_proc, per_str, pct,
        );
    }

    // ── Headroom estimate ─────────────────────────────────────────────────────
    // process_total_ns / PROFILE_ITERS = amortised process cost per sample.
    // periodic_total_ns / PROFILE_ITERS = amortised periodic cost per sample
    //   (periodic fires every COEFF_UPDATE_INTERVAL samples, so its total_ns
    //   across all ticks is already the correct amortised sum).
    let process_ns: u64 = records.iter().map(|r| r.process_total_ns).sum();
    let periodic_ns: u64 = records.iter().map(|r| r.periodic_total_ns).sum();
    // plan_ns_per_frame = total accumulated ns / output frames profiled.
    // Because the inner loop ran OVERSAMPLING_FACTOR times per outer iteration,
    // this already includes the full per-output-frame cost (all inner ticks).
    let plan_ns_per_frame = (process_ns + periodic_ns) as f64 / PROFILE_ITERS as f64;
    let budget_ns = 1_000_000_000.0 / DEVICE_RATE as f64;
    let rt_headroom = budget_ns / plan_ns_per_frame;

    println!();
    println!("oversampling:       {OVERSAMPLING_FACTOR}×  (modules at {SAMPLE_RATE:.0} Hz, output at {DEVICE_RATE:.0} Hz)");
    println!("budget:             {budget_ns:.2} ns/output-frame  (@{DEVICE_RATE} Hz)");
    println!("est. plan cost:     {plan_ns_per_frame:.2} ns/output-frame  ({OVERSAMPLING_FACTOR} inner ticks)");
    println!("estimated headroom: {rt_headroom:.2}×");
}
