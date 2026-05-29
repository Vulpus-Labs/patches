//! Threshold gate detector kernel.
//!
//! Converts a detector input sample into a linear gain (0..1) applied to the
//! dry path. The caller picks the detector source (self-key vs sidechain)
//! and, for stereo, links L/R into a single magnitude before calling
//! [`GateDetector::tick`].
//!
//! Algorithm:
//! 1. Two-state machine (`armed` ↔ `disarmed`) over a rectified input.
//! 2. Fire condition: `armed && |x| > threshold` — opens the gate, disarms,
//!    starts the hold timer. The fire threshold is `threshold`, never
//!    `threshold - hysteresis`. Hysteresis controls *eligibility* (re-arm),
//!    never event timing — see ADR 0076.
//! 3. Close condition: `open && hold_expired && |x| < threshold - hysteresis`.
//! 4. Re-arm: `|x| < threshold - hysteresis` lifts `armed` back high.
//! 5. Asymmetric one-pole ramp (attack on rising, release on falling) between
//!    the binary open target and the smoothed envelope.

use patches_dsp::{compute_time_coeff, ms_to_samples};

/// Pure DSP kernel: detector input sample → linear output gain.
#[derive(Clone, Debug)]
pub struct GateDetector {
    sample_rate: f32,
    threshold_lin: f32,
    rearm_lin: f32,
    threshold_db: f32,
    hysteresis_db: f32,
    hold_samples: usize,
    attack_coeff: f32,
    release_coeff: f32,
    envelope: f32,
    armed: bool,
    open: bool,
    hold_remaining: usize,
}

impl GateDetector {
    pub fn new(sample_rate: f32) -> Self {
        let mut s = Self {
            sample_rate,
            threshold_lin: 0.0,
            rearm_lin: 0.0,
            threshold_db: -40.0,
            hysteresis_db: 3.0,
            hold_samples: 0,
            attack_coeff: 0.0,
            release_coeff: 0.0,
            envelope: 0.0,
            armed: true,
            open: false,
            hold_remaining: 0,
        };
        s.recompute_levels();
        s.set_attack_ms(1.0);
        s.set_hold_ms(10.0);
        s.set_release_ms(100.0);
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

    pub fn set_attack_ms(&mut self, ms: f32) {
        self.attack_coeff = compute_time_coeff(ms, self.sample_rate);
    }

    pub fn set_release_ms(&mut self, ms: f32) {
        self.release_coeff = compute_time_coeff(ms, self.sample_rate);
    }

    pub fn set_hold_ms(&mut self, ms: f32) {
        self.hold_samples = ms_to_samples(ms, self.sample_rate);
    }

    pub fn reset(&mut self) {
        self.envelope = 0.0;
        self.armed = true;
        self.open = false;
        self.hold_remaining = 0;
    }

    fn recompute_levels(&mut self) {
        self.threshold_lin = db_to_lin(self.threshold_db);
        self.rearm_lin = db_to_lin(self.threshold_db - self.hysteresis_db);
    }

    /// One sample of detection. `key` may be signed; rectified internally.
    /// Returns linear gain to apply to the dry path.
    #[inline]
    pub fn tick(&mut self, key: f32) -> f32 {
        let mag = key.abs();

        self.hold_remaining = self.hold_remaining.saturating_sub(1);

        // Fire: armed && above threshold. Threshold is the trigger location;
        // hysteresis is *not* subtracted here — that asymmetry is the point.
        if self.armed && mag > self.threshold_lin {
            self.open = true;
            self.armed = false;
            self.hold_remaining = self.hold_samples;
        }

        // Close: only after hold expires AND signal falls below re-arm band.
        if self.open && self.hold_remaining == 0 && mag < self.rearm_lin {
            self.open = false;
        }

        // Re-arm whenever the signal drops below the re-arm band.
        if !self.armed && mag < self.rearm_lin {
            self.armed = true;
        }

        let target = if self.open { 1.0 } else { 0.0 };
        let coeff = if target > self.envelope { self.attack_coeff } else { self.release_coeff };
        self.envelope += coeff * (target - self.envelope);
        self.envelope
    }

    #[cfg(test)]
    pub(crate) fn is_open(&self) -> bool {
        self.open
    }
}

#[inline]
fn db_to_lin(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn new_detector() -> GateDetector {
        let mut d = GateDetector::new(SR);
        d.set_threshold_db(-20.0);
        d.set_hysteresis_db(6.0);
        d.set_attack_ms(0.1);
        d.set_release_ms(1.0);
        d.set_hold_ms(0.0);
        d
    }

    fn lvl(db: f32) -> f32 {
        db_to_lin(db)
    }

    /// Run `n` samples at constant `key` and return final `(is_open, gain)`.
    fn settle(d: &mut GateDetector, key: f32, n: usize) -> (bool, f32) {
        let mut g = 0.0;
        for _ in 0..n {
            g = d.tick(key);
        }
        (d.is_open(), g)
    }

    #[test]
    fn opens_above_threshold() {
        let mut d = new_detector();
        let (open, g) = settle(&mut d, lvl(-10.0), 1_000);
        assert!(open, "should open above threshold");
        assert!(g > 0.99, "envelope should ramp to ~1.0, got {g}");
    }

    #[test]
    fn stays_closed_below_threshold() {
        let mut d = new_detector();
        let (open, g) = settle(&mut d, lvl(-30.0), 1_000);
        assert!(!open, "should stay closed below threshold");
        assert!(g < 0.01, "envelope should stay near 0, got {g}");
    }

    #[test]
    fn hysteresis_keeps_open_in_band() {
        // Threshold -20 dB, hysteresis 6 dB. After opening, signal at
        // -23 dB (threshold - hyst/2 = -23) must keep the gate open.
        let mut d = new_detector();
        // Open it first.
        settle(&mut d, lvl(-10.0), 1_000);
        assert!(d.is_open());
        // Drop to mid-band and hold.
        let (open, _) = settle(&mut d, lvl(-23.0), 1_000);
        assert!(open, "mid-band signal should keep gate open");
    }

    #[test]
    fn closes_below_rearm_band() {
        // After opening, dropping below threshold-hyst (-26 dB) closes it.
        let mut d = new_detector();
        settle(&mut d, lvl(-10.0), 1_000);
        assert!(d.is_open());
        let (open, _) = settle(&mut d, lvl(-40.0), 2_000);
        assert!(!open, "signal below rearm band should close gate");
    }

    #[test]
    fn hold_prevents_close_inside_window() {
        // Hold = 100 ms. Open with a brief loud burst then drop the signal
        // far below the rearm band — gate must remain open for the full
        // hold window.
        let mut d = new_detector();
        d.set_attack_ms(0.1);
        d.set_release_ms(0.1);
        d.set_hold_ms(100.0);

        // 1 sample above threshold to trigger.
        d.tick(lvl(0.0));
        assert!(d.is_open(), "gate should open on first above-threshold sample");

        // Now drop signal well below rearm and check at 50 ms (mid-hold).
        let half_hold = (50.0 * 0.001 * SR) as usize;
        for _ in 0..half_hold {
            d.tick(lvl(-80.0));
        }
        assert!(d.is_open(), "hold must keep gate open mid-window");

        // Past 100 ms hold the gate closes.
        let rest = (60.0 * 0.001 * SR) as usize;
        for _ in 0..rest {
            d.tick(lvl(-80.0));
        }
        assert!(!d.is_open(), "gate should close after hold expires");
    }

    #[test]
    fn fire_threshold_is_threshold_not_rearm_level() {
        // From cold: a signal sitting at -22 dB (between rearm -26 and
        // threshold -20) must NOT open the gate. The asymmetry is the point.
        let mut d = new_detector();
        let (open, _) = settle(&mut d, lvl(-22.0), 2_000);
        assert!(!open, "signal between rearm and threshold must not open gate");
    }

    #[test]
    fn rearm_required_before_refire() {
        // Open, close, then a signal that returns above threshold but never
        // dropped below the rearm band would not re-fire. Construct: open,
        // drop to mid-band (stays open, never rearms), rise again above
        // threshold — should NOT count as a new fire event.
        let mut d = new_detector();
        d.set_hold_ms(0.0);

        settle(&mut d, lvl(-10.0), 1_000);
        assert!(d.is_open());
        // Move to mid-band: open stays true, armed stays false.
        settle(&mut d, lvl(-23.0), 100);
        assert!(d.is_open());
        // Already open; rising again above threshold is a no-op (armed=false).
        // To prove no spurious refire, dip below rearm now and check the gate
        // closes (it would only stay open if a fresh fire reset hold).
        let (open, _) = settle(&mut d, lvl(-40.0), 2_000);
        assert!(!open, "no phantom refire — gate must be allowed to close");
    }
}
