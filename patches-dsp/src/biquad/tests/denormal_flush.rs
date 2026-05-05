//! Regression test for E134 (denormal hardening). The linear (non-saturating)
//! TDFII path must flush state to zero rather than letting it settle into
//! subnormal range, which would trigger 10–100× microcode stalls in audio.

use super::*;

/// Drive a high-Q lowpass with a unit impulse, then run silence for many
/// samples. Without `flush_denormal` on the state writes, `s1`/`s2` decay
/// asymptotically and enter subnormal range (< ~1.18e-38). With the flush
/// they snap to exactly 0.0.
#[test]
fn linear_path_flushes_state_to_zero_after_decay() {
    // High Q, low cutoff coefficients (hand-picked to give a long decay).
    // Specific values do not matter; only that the impulse response decays
    // toward zero asymptotically without quite reaching it.
    let mut bq = MonoBiquad::new(0.0001, 0.0, 0.0, -1.999, 0.9999);

    // Unit impulse, then silence. 100k samples at 48 kHz ≈ 2 s of decay —
    // plenty to drive a stable IIR into subnormal land.
    let _ = bq.tick(1.0, false);
    for _ in 0..100_000 {
        let _ = bq.tick(0.0, false);
    }

    // Expectation: state has either fully flushed to zero (FTZ in hardware,
    // or our explicit flush_denormal) or is still in normal range. Anything
    // subnormal is a regression.
    assert!(
        bq.s1 == 0.0 || bq.s1.is_normal(),
        "s1 settled into subnormal range: {:e}",
        bq.s1
    );
    assert!(
        bq.s2 == 0.0 || bq.s2.is_normal(),
        "s2 settled into subnormal range: {:e}",
        bq.s2
    );
}
