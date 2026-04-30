//! Microbench for `MonoInput::read` scalar fast path.
//!
//! Run with `cargo run --release --example mono_read_bench -p patches-core`.
//! ADR 0062 / ticket 0768: ensure adding `offset` and `clip` to the input
//! port did not regress the pure-scalar (`offset = 0`, `clip = None`) path.

use patches_core::cables::{CableValue, MonoInput};
use std::hint::black_box;
use std::time::Instant;

const POOL_LEN: usize = 32;
const ITERS: usize = 200_000_000;

fn run(label: &str, input: MonoInput, pool: &[CableValue]) {
    let mut acc = 0.0_f32;
    let start = Instant::now();
    for _ in 0..ITERS {
        acc += black_box(input).read(black_box(pool));
    }
    let elapsed = start.elapsed();
    let ns = elapsed.as_nanos() as f64 / ITERS as f64;
    println!("{label:30} {ns:.3} ns/read  acc={acc}");
}

fn main() {
    let mut pool = vec![CableValue::Mono(0.0); POOL_LEN];
    pool[3] = CableValue::Mono(0.5);

    let scalar = MonoInput::scalar(3, 0.75);
    let affine = MonoInput {
        cable_idx: 3,
        scale: 0.75,
        offset: 0.25,
        clip: None,
        connected: true,
    };
    let clipped = MonoInput {
        cable_idx: 3,
        scale: 0.75,
        offset: 0.25,
        clip: Some((-1.0, 1.0)),
        connected: true,
    };

    // Warm.
    for _ in 0..1_000_000 {
        black_box(scalar.read(&pool));
    }

    run("scalar (offset=0, clip=None)", scalar, &pool);
    run("affine (offset!=0, clip=None)", affine, &pool);
    run("affine + clip", clipped, &pool);
}
