//! Audio-edge detector kernel (ADR 0076).
//!
//! Converts a single audio sample stream into ADR 0047 sub-sample sync events.
//! The fire condition is `armed && prev <= threshold && curr > threshold` for
//! the rising direction (falling and bidirectional follow symmetrically). The
//! sub-sample fraction `frac = (threshold - prev) / (curr - prev)` is the
//! ADR 0047 event fraction, encoding the linear-interpolated zero-crossing
//! position inside the current sample interval.
//!
//! Hysteresis is a two-state machine (`armed` ↔ `disarmed`). Re-arm in the
//! rising direction requires `signal < threshold - hysteresis` (the band is
//! expressed in dB, so `rearm_low_lin = db_to_lin(threshold_db - hysteresis_db)`).
//! **Hysteresis controls eligibility, never event location.** The interp line
//! below uses `threshold`, not `threshold - hysteresis`; the asymmetry is
//! deliberate. Bidirectional detection tracks rising/falling arm states
//! independently.
//!
//! Cooldown suppresses fires for `cooldown_ms` after a fire, even if the
//! signal re-arms; arm state still advances normally during the cooldown
//! window so the first crossing after the window can fire immediately.

use patches_dsp::ms_to_samples;

/// Edge polarity selector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeDirection {
    Rising,
    Falling,
    Both,
}

/// Pure DSP kernel: audio sample → ADR 0047 sub-sample event fraction.
///
/// Returns `0.0` on samples with no event and a value in `(0.0, 1.0]` on
/// samples where a threshold crossing occurred. Stateless across instances,
/// but stateful within (one-sample lookback, two-state arm machine,
/// cooldown counter).
#[derive(Clone, Debug)]
pub struct EdgeDetector {
    sample_rate: f32,
    threshold_db: f32,
    hysteresis_db: f32,
    threshold: f32,
    rearm_low: f32,
    rearm_high: f32,
    direction: EdgeDirection,
    cooldown_samples: usize,

    prev: f32,
    armed_rising: bool,
    armed_falling: bool,
    cooldown_remaining: usize,
}

impl EdgeDetector {
    pub fn new(sample_rate: f32) -> Self {
        let mut s = Self {
            sample_rate,
            threshold_db: -12.0,
            hysteresis_db: 3.0,
            threshold: 0.0,
            rearm_low: 0.0,
            rearm_high: 0.0,
            direction: EdgeDirection::Rising,
            cooldown_samples: 0,
            prev: 0.0,
            armed_rising: true,
            armed_falling: true,
            cooldown_remaining: 0,
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

    pub fn set_direction(&mut self, dir: EdgeDirection) {
        self.direction = dir;
    }

    pub fn set_cooldown_ms(&mut self, ms: f32) {
        self.cooldown_samples = ms_to_samples(ms.max(0.0), self.sample_rate);
    }

    pub fn reset(&mut self) {
        self.prev = 0.0;
        self.armed_rising = true;
        self.armed_falling = true;
        self.cooldown_remaining = 0;
    }

    fn recompute_levels(&mut self) {
        self.threshold = db_to_lin(self.threshold_db);
        self.rearm_low = db_to_lin(self.threshold_db - self.hysteresis_db);
        self.rearm_high = db_to_lin(self.threshold_db + self.hysteresis_db);
    }

    /// One sample of detection. Returns `0.0` on no event, or the ADR 0047
    /// fractional position in `(0.0, 1.0]` on a fire.
    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let prev = self.prev;
        self.prev = x;

        self.cooldown_remaining = self.cooldown_remaining.saturating_sub(1);

        // Re-arm runs unconditionally — hysteresis controls eligibility for
        // the *next* fire, independently of the cooldown debounce.
        if !self.armed_rising && x < self.rearm_low {
            self.armed_rising = true;
        }
        if !self.armed_falling && x > self.rearm_high {
            self.armed_falling = true;
        }

        if self.cooldown_remaining > 0 {
            return 0.0;
        }

        let want_rising =
            matches!(self.direction, EdgeDirection::Rising | EdgeDirection::Both);
        let want_falling =
            matches!(self.direction, EdgeDirection::Falling | EdgeDirection::Both);

        if want_rising && self.armed_rising && prev <= self.threshold && x > self.threshold {
            // Fire at `threshold` — NOT `threshold - hysteresis`. Hysteresis
            // controls eligibility (re-arm), never event location. See ADR 0076.
            let denom = (x - prev).max(f32::EPSILON);
            let frac = ((self.threshold - prev) / denom).clamp(f32::EPSILON, 1.0);
            self.armed_rising = false;
            self.cooldown_remaining = self.cooldown_samples;
            return frac;
        }

        if want_falling && self.armed_falling && prev >= self.threshold && x < self.threshold {
            // Symmetric: fire at `threshold`, not `threshold + hysteresis`.
            let denom = (prev - x).max(f32::EPSILON);
            let frac = ((prev - self.threshold) / denom).clamp(f32::EPSILON, 1.0);
            self.armed_falling = false;
            self.cooldown_remaining = self.cooldown_samples;
            return frac;
        }

        0.0
    }

    #[cfg(test)]
    pub(crate) fn threshold_lin(&self) -> f32 {
        self.threshold
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

    fn rising_detector() -> EdgeDetector {
        let mut d = EdgeDetector::new(SR);
        d.set_threshold_db(-20.0);
        d.set_hysteresis_db(6.0);
        d.set_direction(EdgeDirection::Rising);
        d.set_cooldown_ms(0.0);
        d
    }

    #[test]
    fn linear_interp_fraction_matches_analytic() {
        // Construct a synthetic crossing prev/curr symmetric about threshold.
        // For any threshold T and offset dt: frac = dt / (2*dt) = 0.5 exactly.
        let mut d = rising_detector();
        let t = d.threshold_lin();
        let dt = 0.1_f32;
        d.tick(t - dt); // prime prev — below threshold, no fire
        let frac = d.tick(t + dt);
        assert!(
            (frac - 0.5).abs() < 1e-6,
            "linear-interp fraction wrong: expected 0.5, got {frac}"
        );
    }

    #[test]
    fn linear_interp_asymmetric_crossing() {
        // prev = t - 0.2, curr = t + 0.3 ⇒ frac = 0.2 / 0.5 = 0.4.
        let mut d = rising_detector();
        let t = d.threshold_lin();
        d.tick(t - 0.2);
        let frac = d.tick(t + 0.3);
        assert!(
            (frac - 0.4).abs() < 1e-6,
            "asymmetric interp wrong: expected 0.4, got {frac}"
        );
    }

    #[test]
    fn rearm_with_hysteresis_yields_exactly_one_event() {
        // Oscillate between `threshold - hysteresis / 2` (above rearm band)
        // and `threshold + ε` (above threshold). Only the first upward
        // crossing fires; subsequent oscillations never dip below the rearm
        // band so the detector stays disarmed.
        let mut d = rising_detector();
        let t = d.threshold_lin();
        // Mid-band level: -23 dB ≈ db_to_lin(-23.0). t = db_to_lin(-20).
        let mid = db_to_lin(-23.0);
        let above = t * 1.05;
        let mut fires = 0;
        // Start below rearm band to prime arm state.
        d.tick(db_to_lin(-40.0));
        for _ in 0..200 {
            // Up: mid → above (crosses threshold).
            if d.tick(above) > 0.0 {
                fires += 1;
            }
            // Down: above → mid (still above rearm band; does NOT re-arm).
            d.tick(mid);
        }
        assert_eq!(fires, 1, "expected exactly one event from mid-band oscillation, got {fires}");
    }

    #[test]
    fn cooldown_suppresses_refires_even_when_signal_rearms() {
        let mut d = rising_detector();
        d.set_cooldown_ms(10.0);
        let t = d.threshold_lin();
        let low = db_to_lin(-40.0); // well below rearm band
        let high = t * 1.5;

        // First fire: low → high.
        d.tick(low);
        let first = d.tick(high);
        assert!(first > 0.0, "expected first fire, got {first}");

        // Within cooldown window: bounce signal low and high repeatedly. Signal
        // re-arms (drops below rearm band) but cooldown gates the fire.
        let cooldown_samples = (10.0 * 0.001 * SR) as usize;
        let safe_cycles = (cooldown_samples / 2).saturating_sub(2);
        for _ in 0..safe_cycles {
            d.tick(low);
            let v = d.tick(high);
            assert_eq!(v, 0.0, "cooldown must suppress fires inside window: got {v}");
        }

        // Drain remaining cooldown by holding low (re-arms but cooldown blocks).
        for _ in 0..(cooldown_samples + 4) {
            let v = d.tick(low);
            assert_eq!(v, 0.0, "low ticks should never fire (no upward crossing)");
        }

        // Cooldown expired and armed_rising true (signal stayed below rearm_low).
        let after = d.tick(high);
        assert!(after > 0.0, "expected fire after cooldown expiry, got {after}");
    }

    #[test]
    fn falling_direction_fires_at_threshold_not_at_threshold_plus_hysteresis() {
        // The symmetric trap: a future reader might think the falling fire
        // should happen at `threshold + hysteresis`. It must happen at
        // `threshold`, with `frac = (prev - threshold) / (prev - curr)`.
        let mut d = EdgeDetector::new(SR);
        d.set_threshold_db(-20.0);
        d.set_hysteresis_db(6.0);
        d.set_direction(EdgeDirection::Falling);
        d.set_cooldown_ms(0.0);

        let t = d.threshold_lin();
        // Prime with a sample comfortably above `threshold + hysteresis`.
        d.tick(t * 4.0);
        // Crossing prev → curr where the linear crossing of `threshold` (not
        // `threshold + hysteresis`) happens at frac = 0.5.
        let prev = t + 0.1;
        let curr = t - 0.1;
        d.tick(prev);
        let frac = d.tick(curr);
        let expected = 0.5_f32;
        assert!(
            (frac - expected).abs() < 1e-6,
            "falling interp must fire at threshold: expected {expected}, got {frac}"
        );

        // Same setup but with hysteresis ≠ 0 must still fire at `threshold`,
        // never at `threshold + hysteresis`. Compute what frac would be if the
        // fire (incorrectly) used `threshold + hysteresis`:
        //   wrong_frac = (prev - (T + H)) / (prev - curr)
        // With prev = t + 0.1, curr = t - 0.1, H ≈ db_to_lin(-14)-db_to_lin(-20)
        // the wrong value is materially different from 0.5 — already covered
        // by the equality above.
        let hyst_lin = db_to_lin(-14.0) - t;
        let wrong_frac = ((prev - (t + hyst_lin)) / (prev - curr)).clamp(0.0, 1.0);
        assert!(
            (wrong_frac - frac).abs() > 0.01,
            "kernel must not be using threshold+hysteresis: wrong={wrong_frac}, got={frac}"
        );
    }

    #[test]
    fn bidirectional_fires_both_edges_independently() {
        let mut d = EdgeDetector::new(SR);
        d.set_threshold_db(-20.0);
        d.set_hysteresis_db(6.0);
        d.set_direction(EdgeDirection::Both);
        d.set_cooldown_ms(0.0);
        let t = d.threshold_lin();

        // Prime well below threshold (also below rearm-low band).
        d.tick(db_to_lin(-40.0));
        // Rising fire.
        let f1 = d.tick(t * 2.0);
        assert!(f1 > 0.0, "rising fire expected, got {f1}");
        // Stay above for a bit (does not re-arm rising; falling still armed).
        d.tick(t * 2.0);
        // Falling fire as we drop below threshold (must drop below rearm_high
        // first to arm falling — we never disarmed falling so it fires here).
        let f2 = d.tick(db_to_lin(-40.0));
        assert!(f2 > 0.0, "falling fire expected, got {f2}");
    }

    #[test]
    fn no_fire_when_prev_equals_threshold_and_curr_equals_threshold() {
        // Edge case: prev == curr == threshold. No crossing, no fire.
        let mut d = rising_detector();
        let t = d.threshold_lin();
        d.tick(t);
        let v = d.tick(t);
        assert_eq!(v, 0.0, "no crossing => no event");
    }

    #[test]
    fn rising_direction_ignores_falling_edges() {
        let mut d = rising_detector();
        let t = d.threshold_lin();
        d.tick(t * 2.0);
        let v = d.tick(db_to_lin(-40.0));
        assert_eq!(v, 0.0, "rising-only must not fire on falling edges");
    }
}
