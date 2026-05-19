//! Schmitt-trigger gate kernel (ADR 0076).
//!
//! Sustained gate state from a signed audio sample stream. Opens when
//! `signal > threshold`, closes when `signal < threshold - hysteresis`.
//! Between those bounds the state is held — the defining property of a
//! Schmitt trigger. No sub-sample reporting: gate transitions are
//! sample-accurate per ADR 0030.
//!
//! Shares the threshold + hysteresis levels of
//! [`super::EdgeDetector`](super::EdgeDetector) but with a simpler one-bit
//! state machine (no cooldown, no direction enum, no fractional output).
//! Kept in its own file so kernel tests focus on the schmitt invariants
//! without trigger-family noise.

#[derive(Clone, Debug)]
pub struct GateSchmitt {
    threshold_db: f32,
    hysteresis_db: f32,
    threshold: f32,
    rearm_low: f32,
    open: bool,
}

impl GateSchmitt {
    pub fn new() -> Self {
        let mut s = Self {
            threshold_db: -12.0,
            hysteresis_db: 3.0,
            threshold: 0.0,
            rearm_low: 0.0,
            open: false,
        };
        s.recompute_levels();
        s
    }

    pub fn set_threshold_db(&mut self, db: f32) {
        self.threshold_db = db;
        self.recompute_levels();
    }

    pub fn set_hysteresis_db(&mut self, db: f32) {
        self.hysteresis_db = db.max(0.0);
        self.recompute_levels();
    }

    pub fn reset(&mut self) {
        self.open = false;
    }

    fn recompute_levels(&mut self) {
        self.threshold = db_to_lin(self.threshold_db);
        self.rearm_low = db_to_lin(self.threshold_db - self.hysteresis_db);
    }

    /// One sample of detection. Returns `true` while the gate is open.
    #[inline]
    pub fn tick(&mut self, x: f32) -> bool {
        if self.open {
            if x < self.rearm_low {
                self.open = false;
            }
        } else if x > self.threshold {
            self.open = true;
        }
        self.open
    }

    #[cfg(test)]
    pub(crate) fn threshold_lin(&self) -> f32 {
        self.threshold
    }

    #[cfg(test)]
    pub(crate) fn rearm_low_lin(&self) -> f32 {
        self.rearm_low
    }
}

impl Default for GateSchmitt {
    fn default() -> Self {
        Self::new()
    }
}

#[inline]
fn db_to_lin(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rising_gate() -> GateSchmitt {
        let mut g = GateSchmitt::new();
        g.set_threshold_db(-20.0);
        g.set_hysteresis_db(6.0);
        g
    }

    #[test]
    fn opens_on_rising_crossing_closes_below_rearm() {
        let mut g = rising_gate();
        let t = g.threshold_lin();
        let r = g.rearm_low_lin();
        assert!(!g.tick(0.0));
        assert!(!g.tick(t * 0.5), "below threshold => closed");
        assert!(g.tick(t * 1.5), "above threshold => open");
        assert!(g.tick(t * 1.1), "still above threshold => open");
        assert!(g.tick(r * 1.5), "above rearm_low but below threshold => stays open");
        assert!(!g.tick(r * 0.5), "below rearm_low => closes");
    }

    #[test]
    fn small_oscillation_in_hysteresis_band_does_not_toggle() {
        // Oscillate strictly inside (rearm_low, threshold) — the schmitt band.
        // With gate starting closed, it must never open. With gate forced open
        // and then oscillated, it must never close.
        let mut g = rising_gate();
        let t = g.threshold_lin();
        let r = g.rearm_low_lin();
        // Pick two points just inside the band.
        let lo = r + 0.1 * (t - r);
        let hi = t - 0.1 * (t - r);
        // Closed branch: cannot open without ever exceeding threshold.
        for _ in 0..200 {
            assert!(!g.tick(lo));
            assert!(!g.tick(hi));
        }
        // Force open then oscillate inside band.
        g.tick(t * 2.0);
        assert!(g.tick(t * 2.0));
        for _ in 0..200 {
            assert!(g.tick(lo), "inside band must not close");
            assert!(g.tick(hi), "inside band must not close");
        }
    }

    #[test]
    fn signed_semantics_negative_does_not_open() {
        // Threshold is signed, not magnitude. A symmetric oscillator at
        // ±0.5 with threshold 0.1 should open on the positive half-cycle
        // only; the negative half-cycle is below `rearm_low = 0.1 - eps`
        // so the gate closes again. This is the documented behaviour.
        let mut g = GateSchmitt::new();
        g.set_threshold_db(-20.0); // ≈ 0.1 linear
        g.set_hysteresis_db(6.0);
        let mut transitions = 0;
        let mut prev = false;
        for i in 0..1000 {
            let x = 0.5 * (std::f32::consts::TAU * 100.0 / 48_000.0 * i as f32).sin();
            let now = g.tick(x);
            if now != prev {
                transitions += 1;
            }
            prev = now;
        }
        // ~100 Hz oscillator at 48 kHz = ~2.08 cycles. Each cycle = 2 transitions
        // (open + close). Allow ±2 for end-effects.
        assert!(
            (4..=6).contains(&transitions),
            "expected ~4-5 transitions on signed schmitt, got {transitions}"
        );
    }

    #[test]
    fn idempotent_when_signal_held_above_threshold() {
        let mut g = rising_gate();
        let t = g.threshold_lin();
        for _ in 0..1000 {
            assert!(g.tick(t * 2.0));
        }
    }

    #[test]
    fn reset_clears_open_state() {
        let mut g = rising_gate();
        g.tick(g.threshold_lin() * 2.0);
        assert!(g.tick(g.threshold_lin() * 2.0));
        g.reset();
        // After reset, signal must cross threshold again to re-open.
        // A sample inside the band (above rearm_low) must not auto-open.
        let between = (g.threshold_lin() + g.rearm_low_lin()) * 0.5;
        assert!(!g.tick(between));
    }

    #[test]
    fn hysteresis_zero_collapses_to_single_threshold() {
        let mut g = GateSchmitt::new();
        g.set_threshold_db(-20.0);
        g.set_hysteresis_db(0.0);
        let t = g.threshold_lin();
        assert!(g.tick(t * 1.5));
        assert!(!g.tick(t * 0.5), "zero hysteresis: close immediately below threshold");
    }
}
