//! One-pole highpass filter for DC removal.
//!
//! Implements a simple DC blocker with a cutoff around 5 Hz (configurable
//! via sample rate). Useful after waveshapers, bias injection, or any
//! process that may introduce a DC offset.
//!
//! ```text
//! y[n] = x[n] - x[n-1] + R * y[n-1]
//! R = 1 - 2*pi*fc / sample_rate
//! ```

use std::f32::consts::TAU;

/// One-pole highpass at ~5 Hz for DC removal.
#[derive(Clone)]
pub struct DcBlocker {
    x_prev: f32,
    y_prev: f32,
    r: f32,
}

impl DcBlocker {
    /// Create a new DC blocker tuned to the given sample rate.
    pub fn new(sample_rate: f32) -> Self {
        Self {
            x_prev: 0.0,
            y_prev: 0.0,
            r: 1.0 - TAU * 5.0 / sample_rate,
        }
    }

    /// Process one sample, returning the DC-blocked output.
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = crate::flush_denormal(x - self.x_prev + self.r * self.y_prev);
        self.x_prev = x;
        self.y_prev = y;
        y
    }

    /// Reset internal state to zero.
    pub fn reset(&mut self) {
        self.x_prev = 0.0;
        self.y_prev = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 44_100.0;

    #[test]
    fn removes_dc_offset() {
        let mut dc = DcBlocker::new(SR);
        // Feed constant DC for a while
        for _ in 0..44_100 {
            dc.process(1.0);
        }
        // Output should have settled near zero
        let out = dc.process(1.0);
        assert!(out.abs() < 0.01, "DC blocker should remove constant offset, got {out}");
    }

    #[test]
    fn passes_ac_signal() {
        let mut dc = DcBlocker::new(SR);
        // Warm up
        for i in 0..4410 {
            let x = (i as f32 * 0.1).sin();
            dc.process(x);
        }
        // AC signal should pass through with minimal attenuation
        let x = (4410.0_f32 * 0.1).sin();
        let out = dc.process(x);
        assert!(out.abs() > 0.1, "DC blocker should pass AC, got {out}");
    }

    #[test]
    fn prolonged_silence_does_not_produce_denormals() {
        let mut dc = DcBlocker::new(SR);
        // Kick with a DC pulse, then run silence for 30 seconds
        for _ in 0..100 {
            dc.process(1.0);
        }
        for i in 0..(SR as usize * 30) {
            let out = dc.process(0.0);
            assert!(
                out == 0.0 || out.is_normal(),
                "denormal at sample {i}: {out:e} (bits: {:#010x})",
                out.to_bits()
            );
        }
    }

    #[test]
    fn reset_clears_state() {
        let mut dc = DcBlocker::new(SR);
        dc.process(1.0);
        dc.process(1.0);
        dc.reset();
        assert_eq!(dc.x_prev, 0.0);
        assert_eq!(dc.y_prev, 0.0);
    }
}
