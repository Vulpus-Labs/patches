/// ADSR stage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AdsrStage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// Core ADSR state machine with linear ramp arithmetic.
/// No dependency on patches-core or patches-modules.
pub struct AdsrCore {
    pub stage: AdsrStage,
    pub level: f32,
    attack_inc: f32,
    decay_inc: f32,
    sustain: f32,
    release_secs: f32,
    release_inc: f32,
    sample_rate: f32,
}

impl AdsrCore {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            stage: AdsrStage::Idle,
            level: 0.0,
            attack_inc: 0.0,
            decay_inc: 0.0,
            sustain: 0.0,
            release_secs: 0.0,
            release_inc: 0.0,
            sample_rate,
        }
    }

    /// Recompute increments from the given ADSR parameters.
    pub fn set_params(&mut self, attack_secs: f32, decay_secs: f32, sustain: f32, release_secs: f32) {
        self.attack_inc = 1.0 / (attack_secs * self.sample_rate);
        self.sustain = sustain;
        self.decay_inc = (1.0 - sustain) / (decay_secs * self.sample_rate);
        self.release_secs = release_secs;
        // release_inc is recomputed on entry to Release using the current level
    }

    /// Reset to Idle state.
    pub fn reset(&mut self) {
        self.stage = AdsrStage::Idle;
        self.level = 0.0;
    }

    /// Transition to Release from the current level.
    fn enter_release(&mut self) {
        self.release_inc = self.level / (self.release_secs * self.sample_rate);
        self.level -= self.release_inc;
        if self.level <= 0.0 {
            self.level = 0.0;
            self.stage = AdsrStage::Idle;
        } else {
            self.stage = AdsrStage::Release;
        }
    }

    /// Run one sample of the ADSR state machine.
    ///
    /// `triggered` should be `true` on the single sample where a rising edge
    /// was detected (via `TriggerInput::tick`). `gate_high` should be `true`
    /// while the gate signal is above threshold (via `GateInput::tick().is_high`).
    ///
    /// Returns the envelope level clamped to [0, 1].
    pub fn tick(&mut self, triggered: bool, gate_high: bool) -> f32 {
        // Rising trigger: restart Attack from any state and current level
        if triggered {
            self.stage = AdsrStage::Attack;
        }

        match self.stage {
            AdsrStage::Idle => {}
            AdsrStage::Attack => {
                if !gate_high {
                    self.enter_release();
                } else {
                    self.level += self.attack_inc;
                    if self.level >= 1.0 {
                        self.level = 1.0;
                        self.stage = AdsrStage::Decay;
                    }
                }
            }
            AdsrStage::Decay => {
                if !gate_high {
                    self.enter_release();
                } else {
                    self.level -= self.decay_inc;
                    if self.level <= self.sustain {
                        self.level = self.sustain;
                        self.stage = AdsrStage::Sustain;
                    }
                }
            }
            AdsrStage::Sustain => {
                self.level = self.sustain;
                if !gate_high {
                    self.enter_release();
                }
            }
            AdsrStage::Release => {
                self.level -= self.release_inc;
                if self.level <= 0.0 {
                    self.level = 0.0;
                    self.stage = AdsrStage::Idle;
                }
            }
        }

        self.level.clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{assert_within, assert_reset_deterministic};

    fn make_core(attack: f32, decay: f32, sustain: f32, release: f32, sample_rate: f32) -> AdsrCore {
        let mut c = AdsrCore::new(sample_rate);
        c.set_params(attack, decay, sustain, release);
        c
    }

    /// T4 — stability and convergence: rapid gate toggling produces no NaN or out-of-range values.
    #[test]
    fn t4_rapid_gate_toggling_no_nan_or_out_of_range() {
        let mut core = make_core(0.01, 0.01, 0.5, 0.01, 44100.0);

        for _ in 0..50 {
            // Trigger + gate high: enter attack (only first sample is a trigger)
            {
                let v = core.tick(true, true);
                assert!(v.is_finite(), "NaN or inf during trigger: {v}");
                assert!((0.0..=1.0).contains(&v), "out of range during trigger: {v}");
            }
            for _ in 0..2 {
                let v = core.tick(false, true);
                assert!(v.is_finite(), "NaN or inf during gate-high: {v}");
                assert!((0.0..=1.0).contains(&v), "out of range during gate-high: {v}");
            }
            // Gate low: release
            for _ in 0..3 {
                let v = core.tick(false, false);
                assert!(v.is_finite(), "NaN or inf during gate-low: {v}");
                assert!((0.0..=1.0).contains(&v), "out of range during gate-low: {v}");
            }
        }
    }

    /// T5 — linearity: attack ramp increments are constant.
    #[test]
    fn t5_attack_ramp_is_linear() {
        // attack=0.5s at 10 Hz → 5 samples, inc=0.2
        let mut core = make_core(0.5, 1.0, 0.5, 0.5, 10.0);

        // Trigger the attack
        let v0 = core.tick(true, true);
        let v1 = core.tick(false, true);
        let v2 = core.tick(false, true);
        let v3 = core.tick(false, true);

        let d0 = v1 - v0;
        let d1 = v2 - v1;
        let d2 = v3 - v2;

        assert_within!(d0, d1, 1e-5, "attack not linear: d0={d0}, d1={d1}");
        assert_within!(d1, d2, 1e-5, "attack not linear: d1={d1}, d2={d2}");
    }

    /// T5 — linearity: decay ramp slope is constant.
    #[test]
    fn t5_decay_ramp_is_linear() {
        // attack=0.1s (1 sample), decay=0.5s (5 samples), sustain=0.5
        // decay_inc = (1.0 - 0.5) / (0.5 * 10) = 0.1
        let mut core = make_core(0.1, 0.5, 0.5, 0.5, 10.0);

        // Complete attack in 1 sample
        let _v_attack = core.tick(true, true);

        // Now in decay: collect values
        let d0 = core.tick(false, true);
        let d1 = core.tick(false, true);
        let d2 = core.tick(false, true);
        let d3 = core.tick(false, true);

        let diff0 = d0 - d1;
        let diff1 = d1 - d2;
        let diff2 = d2 - d3;

        assert_within!(diff0, diff1, 1e-5, "decay not linear: diff0={diff0}, diff1={diff1}");
        assert_within!(diff1, diff2, 1e-5, "decay not linear: diff1={diff1}, diff2={diff2}");
    }

    /// Envelope shape: attack peaks at 1.0, sustain holds at sustain level,
    /// release decays to 0.0.
    #[test]
    fn envelope_shape_attack_sustain_release() {
        let sr = 48_000.0;
        let attack = 0.01;   // 480 samples
        let decay = 0.02;    // 960 samples
        let sustain = 0.5;
        let release = 0.03;  // 1440 samples
        let mut core = make_core(attack, decay, sustain, release, sr);

        let attack_samples = (attack * sr) as usize;
        let decay_samples = (decay * sr) as usize;
        let sustain_samples = 500;
        let release_samples = (release * sr) as usize;

        // Attack phase: trigger + gate high
        let mut peak = 0.0f32;
        let v = core.tick(true, true); // trigger fires
        if v > peak { peak = v; }
        for _ in 1..attack_samples + 10 {
            let v = core.tick(false, true);
            if v > peak { peak = v; }
        }
        assert!(
            (peak - 1.0).abs() < 1e-3,
            "attack should reach 1.0, peak was {peak}"
        );

        // Decay into sustain
        for _ in 0..decay_samples + 10 {
            core.tick(false, true);
        }

        // Sustain phase: hold gate high
        for _ in 0..sustain_samples {
            let v = core.tick(false, true);
            assert!(
                (v - sustain).abs() < 1e-3,
                "sustain should hold at {sustain}, got {v}"
            );
        }

        // Release phase: drop gate
        for _ in 0..release_samples + 100 {
            core.tick(false, false);
        }
        let final_val = core.tick(false, false);
        assert!(
            final_val.abs() < 1e-3,
            "release should reach 0.0, got {final_val}"
        );
        assert_eq!(core.stage, AdsrStage::Idle);
    }

    /// Re-triggering during release restarts attack from the current level.
    #[test]
    fn retrigger_during_release_restarts_from_current_level() {
        let sr = 48_000.0;
        let mut core = make_core(0.01, 0.01, 0.5, 0.05, sr);

        // Trigger, let it reach sustain
        core.tick(true, true);
        for _ in 0..5000 {
            core.tick(false, true);
        }

        // Release
        for _ in 0..500 {
            core.tick(false, false);
        }
        let level_before_retrigger = core.level;
        assert!(
            level_before_retrigger > 0.0 && level_before_retrigger < 0.5,
            "should be mid-release, level={level_before_retrigger}"
        );

        // Re-trigger: attack should start from current level, not 0
        let v = core.tick(true, true);
        assert_eq!(core.stage, AdsrStage::Attack);
        assert!(
            v >= level_before_retrigger,
            "after retrigger, level {v} should be >= pre-retrigger level {level_before_retrigger}"
        );
    }

    /// A very short gate (shorter than attack time) peaks below 1.0.
    #[test]
    fn short_gate_peaks_below_one() {
        let sr = 48_000.0;
        let attack = 0.05; // 2400 samples
        let mut core = make_core(attack, 0.01, 0.5, 0.03, sr);

        // Trigger and hold gate for only 100 samples (well within attack phase)
        core.tick(true, true);
        let mut peak = core.level;
        for _ in 1..100 {
            let v = core.tick(false, true);
            if v > peak { peak = v; }
        }
        assert!(peak < 1.0, "short gate should not reach 1.0, peak was {peak}");
        assert!(peak > 0.0, "short gate should produce some output");

        // Release: let the envelope decay
        for _ in 0..5000 {
            core.tick(false, false);
        }
        let final_val = core.tick(false, false);
        assert!(
            final_val.abs() < 1e-3,
            "should release to 0.0, got {final_val}"
        );
    }

    /// T7 — determinism: reset() produces bit-identical output on repeated runs.
    #[test]
    fn t7_reset_produces_identical_output() {
        let inputs: [(bool, bool); 7] = [
            (true, true),
            (false, true),
            (false, true),
            (false, false),
            (false, false),
            (true, true),
            (false, true),
        ];

        assert_reset_deterministic!(
            make_core(0.5, 0.5, 0.5, 0.5, 10.0),
            &inputs,
            |core: &mut AdsrCore, (t, g): (bool, bool)| core.tick(t, g),
            |core: &mut AdsrCore| core.reset()
        );
    }
}
