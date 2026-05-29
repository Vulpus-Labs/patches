//! Regression test for E134 (denormal hardening). During long silence the
//! biquad TDFII state recurrence decays asymptotically and enters subnormal
//! range (< ~1.18e-38), which on x86 triggers 10–100× microcode stalls — CPU
//! rising as a sound dies away.
//!
//! Ticket 0954 made `FtzGuard` the single owner of hardware flush-to-zero at
//! the block boundary; ticket 0962 then deleted the per-site `flush_denormal`
//! calls that used to scrub the state inline. This test pins the surviving
//! guarantee: that an `FtzGuard` actually flushes the decaying state on this
//! path — rather than asserting (as before) that the kernel scrubs itself.
//!
//! A plain value assertion would not discriminate: without FTZ the state still
//! reaches zero *eventually* via gradual underflow, just after a long subnormal
//! stretch (the slow stretch is the whole problem). So we instead locate a
//! sample index where, with no FTZ, the state is a nonzero subnormal, and prove
//! that under an `FtzGuard` that very sample is already exactly `0.0`. If the
//! guard stopped setting FTZ/DAZ, the assertion would fail.

use super::*;

/// Returns `true` for a subnormal, nonzero `f32` — the values FTZ flushes and
/// gradual underflow lingers on.
fn is_subnormal_nonzero(x: f32) -> bool {
    x != 0.0 && !x.is_normal()
}

/// Resonator with conjugate poles at radius r = sqrt(0.81) = 0.9 (a1 = -2·r·cos θ,
/// a2 = r²). Decays ~0.9/sample, crossing into subnormal range in ~850 samples.
fn resonator() -> MonoBiquad {
    MonoBiquad::new(1.0, 0.0, 0.0, -1.782, 0.81)
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn ftz_guard_flushes_subnormal_state_on_linear_path() {
    // Reference pass with no guard (process default is FTZ-off): find the first
    // silent-sample index where the decaying state is a nonzero subnormal.
    let mut plain = resonator();
    let _ = plain.tick(1.0, false);
    let mut subnormal_at = None;
    for n in 1..=20_000 {
        let _ = plain.tick(0.0, false);
        if is_subnormal_nonzero(plain.s1) {
            subnormal_at = Some(n);
            break;
        }
    }
    let subnormal_at = subnormal_at
        .expect("resonator state never entered subnormal range without FTZ");

    // Same filter, same input, same sample count — but under an FtzGuard the
    // subnormal result is flushed to exactly zero the moment it appears.
    let _ftz = crate::FtzGuard::enable();
    let mut guarded = resonator();
    let _ = guarded.tick(1.0, false);
    for _ in 0..subnormal_at {
        let _ = guarded.tick(0.0, false);
    }

    assert_eq!(
        guarded.s1, 0.0,
        "FtzGuard did not flush s1 (subnormal {:e} without FTZ at sample {subnormal_at})",
        plain.s1
    );
}
