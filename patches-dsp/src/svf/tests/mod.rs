//! Tests for the Chamberlin SVF kernel. Split by family from the original
//! `tests.rs` per ticket 0539. Shared fixtures and helpers live here;
//! behaviour-specific tests live in sibling submodules.

#![allow(unused_imports)]

pub(super) use super::*;

use crate::test_support::assert_within;
use std::f32::consts::PI;

pub(super) const SAMPLE_RATE: f32 = 48_000.0;

pub(super) fn make_kernel(cutoff_hz: f32, q_norm: f32) -> SvfKernel {
    let f = svf_f(cutoff_hz, SAMPLE_RATE);
    let d = q_to_damp(q_norm);
    SvfKernel::new_static(f, d)
}

pub(super) fn db(ratio: f32) -> f32 {
    20.0 * ratio.log10()
}

pub(super) fn measure_steady_state_amplitude(
    kernel: &mut SvfKernel,
    freq_hz: f32,
    mode_fn: fn((f32, f32, f32)) -> f32,
) -> f32 {
    let omega = 2.0 * PI * freq_hz / SAMPLE_RATE;
    // Warm-up: 4096 samples to reach steady state
    for i in 0..4096_usize {
        let x = (omega * i as f32).sin();
        let out = kernel.tick(x);
        let _ = mode_fn(out);
    }
    // Measurement: accumulate peak over 1024 samples
    let mut peak = 0.0_f32;
    for i in 4096..5120_usize {
        let x = (omega * i as f32).sin();
        let out = kernel.tick(x);
        let y = mode_fn(out);
        if y.abs() > peak {
            peak = y.abs();
        }
    }
    peak
}

mod dc_nyquist;
mod frequency_response;
mod impulse;
mod quality;
mod stability;
