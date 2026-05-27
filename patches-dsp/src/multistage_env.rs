//! Multi-breakpoint envelope state machine.
//!
//! Sibling to [`AdsrCore`](crate::adsr::AdsrCore), but instead of a fixed
//! attack/decay/sustain/release shape it runs an arbitrary number of
//! `(target_level, time, curve)` stages with a designated *sustain stage*.
//! Stages `0..=sustain_stage` form the pre-sustain contour (e.g. an attack
//! spike → dip → secondary swell); the machine holds at the sustain stage's
//! level while the gate is high, then runs stages `sustain_stage+1..len` as the
//! release tail on gate-off. This expresses D50-style contours that ADSR
//! cannot.
//!
//! The core is pure and alloc-free (fixed-capacity stage storage), so it is
//! audio-thread safe. Time-scaling (key-follow) and velocity scaling are folded
//! in by the *module* and passed to the core as already-scaled inputs:
//! `time_scale` multiplies every stage time per tick, and the latched
//! level-scale (set via [`EnvCore::set_level_scale`]) multiplies stage targets.
//! The core never knows about pitch or velocity directly.

use crate::adsr::AdsrShape;

/// Maximum number of stages an [`EnvCore`] can hold. Eight is comfortably
/// enough for D50-style contours; it also becomes the descriptor count-axis
/// bound for the `Env` module (ticket 0952).
pub const MAX_STAGES: usize = 8;

/// Time-constant convention for exponential segments: a stage's nominal time
/// spans this many τ, so at the end of a full-length exponential stage the
/// level has approached ~(1 − e⁻⁵) ≈ 99.3 % of the target. Mirrors the
/// `EXP_N_TAU` convention in [`crate::adsr`] so slider feel is compatible.
const EXP_N_TAU: f32 = 5.0;

/// A stage runs the level from its entry value toward `target_level` over
/// `time_secs` seconds (before `time_scale` is applied), following `curve`.
///
/// `curve` reuses [`AdsrShape`]: `Linear` is a constant-slope ramp,
/// `Exponential` is an RC-style asymptotic approach.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stage {
    pub target_level: f32,
    pub time_secs: f32,
    pub curve: AdsrShape,
}

impl Stage {
    pub fn new(target_level: f32, time_secs: f32, curve: AdsrShape) -> Self {
        Self { target_level, time_secs, curve }
    }
}

impl Default for Stage {
    fn default() -> Self {
        Self { target_level: 0.0, time_secs: 0.0, curve: AdsrShape::Linear }
    }
}

/// Coarse run-state of an [`EnvCore`], exposed for inspection/testing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvPhase {
    /// Not running. Level holds at whatever it last reached.
    Idle,
    /// Ramping through a stage (pre-sustain contour or release tail).
    Active,
    /// Held at the sustain stage's level while the gate is high.
    Sustain,
}

/// Threshold below which a stage's effective sample-count is treated as zero,
/// producing an instant jump to the target.
const INSTANT_EPS: f32 = 1.0e-9;

/// Multi-stage envelope core. No dependency on `patches-core` or
/// `patches-modules`; no heap allocation.
pub struct EnvCore {
    stages: [Stage; MAX_STAGES],
    len: usize,
    /// Index of the stage whose target level is held while the gate is high.
    /// Clamped into `0..len` by the setters.
    sustain_stage: usize,
    sample_rate: f32,

    phase_state: EnvPhase,
    /// Index of the stage currently being ramped (valid when `Active`).
    current_stage: usize,
    /// Progress within the current stage, `0.0..1.0`.
    phase: f32,
    /// Level at entry to the current stage — the ramp's start point. Latched on
    /// stage entry so re-triggers ramp from the *current* level (no click).
    seg_start: f32,
    /// Current output level.
    pub level: f32,

    /// Level scale set by the module (velocity). Latched into
    /// `latched_level_scale` at each trigger so an in-flight envelope keeps a
    /// stable velocity scaling.
    level_scale: f32,
    latched_level_scale: f32,
}

impl EnvCore {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            stages: [Stage::default(); MAX_STAGES],
            len: 0,
            sustain_stage: 0,
            sample_rate,
            phase_state: EnvPhase::Idle,
            current_stage: 0,
            phase: 0.0,
            seg_start: 0.0,
            level: 0.0,
            level_scale: 1.0,
            latched_level_scale: 1.0,
        }
    }

    /// Replace the stage list. Copies up to [`MAX_STAGES`] entries; extra stages
    /// are ignored. The sustain-stage index is re-clamped into the new range.
    pub fn set_stages(&mut self, stages: &[Stage]) {
        self.len = stages.len().min(MAX_STAGES);
        self.stages[..self.len].copy_from_slice(&stages[..self.len]);
        self.clamp_sustain_stage();
    }

    /// Designate the sustain stage. Clamped into `0..len`.
    pub fn set_sustain_stage(&mut self, index: usize) {
        self.sustain_stage = index;
        self.clamp_sustain_stage();
    }

    fn clamp_sustain_stage(&mut self) {
        let max = self.len.saturating_sub(1);
        if self.sustain_stage > max {
            self.sustain_stage = max;
        }
    }

    /// Set the per-trigger level scale (velocity). Stored now; latched and
    /// applied to stage targets at the next trigger. Negative values are
    /// clamped to zero.
    pub fn set_level_scale(&mut self, scale: f32) {
        self.level_scale = scale.max(0.0);
    }

    /// Reset to idle at level 0.
    pub fn reset(&mut self) {
        self.phase_state = EnvPhase::Idle;
        self.current_stage = 0;
        self.phase = 0.0;
        self.seg_start = 0.0;
        self.level = 0.0;
    }

    /// Coarse run-state, for inspection.
    pub fn phase_state(&self) -> EnvPhase {
        self.phase_state
    }

    /// Stage target after the latched velocity scaling.
    fn effective_target(&self, stage: usize) -> f32 {
        self.stages[stage].target_level * self.latched_level_scale
    }

    /// Begin the contour from stage 0, ramping from the current level.
    fn start(&mut self) {
        if self.len == 0 {
            self.phase_state = EnvPhase::Idle;
            return;
        }
        self.latched_level_scale = self.level_scale;
        self.current_stage = 0;
        self.phase = 0.0;
        self.seg_start = self.level;
        self.phase_state = EnvPhase::Active;
    }

    /// Begin the release tail (stages after the sustain stage). If there is no
    /// stage past the sustain stage the envelope simply idles at its current
    /// level — designate a final stage with target `0.0` for a release to
    /// silence.
    fn enter_release(&mut self) {
        let next = self.sustain_stage + 1;
        if next >= self.len {
            self.phase_state = EnvPhase::Idle;
        } else {
            self.current_stage = next;
            self.phase = 0.0;
            self.seg_start = self.level;
            self.phase_state = EnvPhase::Active;
        }
    }

    /// Run one sample.
    ///
    /// `triggered` is `true` only on the rising-edge sample. `gate_high` is
    /// `true` while the gate is above threshold. `time_scale` multiplies every
    /// stage time this tick (key-follow and any global rate scaling, computed
    /// by the module). Returns the level clamped to `[0, 1]`.
    pub fn tick(&mut self, triggered: bool, gate_high: bool, time_scale: f32) -> f32 {
        if triggered {
            self.start();
        }

        // Gate-off transitions into the release tail.
        match self.phase_state {
            // Gate dropped while held at sustain.
            EnvPhase::Sustain if !gate_high => self.enter_release(),
            // Gate dropped mid pre-sustain ramp — jump straight to release,
            // mirroring AdsrCore's enter-release-from-any-stage behaviour.
            EnvPhase::Active if self.current_stage <= self.sustain_stage && !gate_high => {
                self.enter_release()
            }
            _ => {}
        }

        match self.phase_state {
            EnvPhase::Active => self.advance(time_scale, gate_high),
            EnvPhase::Sustain => self.level = self.effective_target(self.sustain_stage),
            EnvPhase::Idle => {}
        }

        self.level = self.level.clamp(0.0, 1.0);
        self.level
    }

    /// Advance the active stage by one sample. Chains instantly through
    /// zero-time stages within the same tick; a normal stage consumes the tick
    /// and the next stage starts ramping on the following tick.
    fn advance(&mut self, time_scale: f32, gate_high: bool) {
        // Bounded: each iteration either returns or steps the stage forward,
        // and there are at most `len` stages.
        for _ in 0..=self.len {
            let stage = self.stages[self.current_stage];
            let target = self.effective_target(self.current_stage);
            let samples = stage.time_secs * time_scale * self.sample_rate;

            if samples <= INSTANT_EPS {
                // Zero-time stage: jump to target and chain to the next stage
                // without consuming this tick's sample.
                self.level = target;
                if !self.complete_stage(gate_high) {
                    return;
                }
                continue;
            }

            self.phase += 1.0 / samples;
            if self.phase >= 1.0 {
                self.level = target;
                self.complete_stage(gate_high);
            } else {
                self.level = interp(self.seg_start, target, self.phase, stage.curve);
            }
            return;
        }
    }

    /// Handle completion of `current_stage`. Returns `true` if the machine
    /// advanced to another `Active` stage (so an instant-stage chain should
    /// continue), `false` if it entered `Sustain` or `Idle`.
    fn complete_stage(&mut self, gate_high: bool) -> bool {
        if self.current_stage == self.sustain_stage && gate_high {
            self.phase_state = EnvPhase::Sustain;
            return false;
        }
        let next = self.current_stage + 1;
        if next >= self.len {
            self.phase_state = EnvPhase::Idle;
            return false;
        }
        self.current_stage = next;
        self.phase = 0.0;
        self.seg_start = self.level;
        true
    }
}

/// Interpolate from `start` to `target` at progress `phase` in `0..1`.
fn interp(start: f32, target: f32, phase: f32, curve: AdsrShape) -> f32 {
    match curve {
        AdsrShape::Linear => start + (target - start) * phase,
        // RC-style approach: at phase=1 we've covered ~(1 − e⁻⁵) of the span.
        AdsrShape::Exponential => {
            let frac = 1.0 - (-EXP_N_TAU * phase).exp();
            start + (target - start) * frac
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{assert_within, assert_reset_deterministic};

    fn lin(target: f32, time: f32) -> Stage {
        Stage::new(target, time, AdsrShape::Linear)
    }

    /// Single stage with sustain at stage 0 = a linear ramp to the target,
    /// then a hold.
    #[test]
    fn single_stage_is_linear_ramp() {
        // sr=10, time=0.5s → 5 samples, inc=0.2 toward 1.0.
        let mut env = EnvCore::new(10.0);
        env.set_stages(&[lin(1.0, 0.5)]);
        env.set_sustain_stage(0);

        let v0 = env.tick(true, true, 1.0);
        let v1 = env.tick(false, true, 1.0);
        let v2 = env.tick(false, true, 1.0);
        assert_within!(0.2, v0, 1e-6);
        let d0 = v1 - v0;
        let d1 = v2 - v1;
        assert_within!(d0, d1, 1e-6, "ramp not linear: d0={d0}, d1={d1}");
    }

    /// Holds at the sustain stage's level while the gate is high.
    #[test]
    fn sustain_holds_under_gate() {
        let mut env = EnvCore::new(10.0);
        env.set_stages(&[lin(0.8, 0.5)]);
        env.set_sustain_stage(0);

        env.tick(true, true, 1.0);
        for _ in 0..20 {
            env.tick(false, true, 1.0);
        }
        assert_eq!(env.phase_state(), EnvPhase::Sustain);
        for _ in 0..50 {
            let v = env.tick(false, true, 1.0);
            assert_within!(0.8, v, 1e-6, "sustain should hold at 0.8, got {v}");
        }
    }

    /// Gate-off runs the remaining stages as the release tail, down to idle.
    #[test]
    fn release_tail_on_gate_off() {
        // Stage 0 (sustain): ramp to 1.0 over 5 samples.
        // Stage 1 (release): ramp to 0.0 over 5 samples.
        let mut env = EnvCore::new(10.0);
        env.set_stages(&[lin(1.0, 0.5), lin(0.0, 0.5)]);
        env.set_sustain_stage(0);

        env.tick(true, true, 1.0);
        for _ in 0..20 {
            env.tick(false, true, 1.0);
        }
        assert_eq!(env.phase_state(), EnvPhase::Sustain);
        assert_within!(1.0, env.level, 1e-6);

        // First gate-off sample begins the release immediately.
        let r0 = env.tick(false, false, 1.0);
        assert_within!(0.8, r0, 1e-6, "release should start immediately, got {r0}");
        for _ in 0..20 {
            env.tick(false, false, 1.0);
        }
        assert_eq!(env.phase_state(), EnvPhase::Idle);
        assert_within!(0.0, env.level, 1e-6);
    }

    /// `time_scale` stretches/compresses stage durations.
    #[test]
    fn time_scale_scales_durations() {
        let count_to_sustain = |scale: f32| -> usize {
            let mut env = EnvCore::new(10.0);
            env.set_stages(&[lin(1.0, 0.4)]); // nominal 4 samples
            env.set_sustain_stage(0);
            let mut n = 0;
            env.tick(true, true, scale);
            n += 1;
            while env.phase_state() != EnvPhase::Sustain && n < 1000 {
                env.tick(false, true, scale);
                n += 1;
            }
            n
        };
        let base = count_to_sustain(1.0);
        let slow = count_to_sustain(2.0);
        let fast = count_to_sustain(0.5);
        assert_eq!(slow, base * 2, "scale 2.0 should double duration");
        assert_eq!(fast, base / 2, "scale 0.5 should halve duration");
    }

    /// Re-trigger mid-ramp restarts from the current level (no jump to 0).
    #[test]
    fn retrigger_from_mid_level() {
        let mut env = EnvCore::new(10.0);
        env.set_stages(&[lin(1.0, 0.5)]);
        env.set_sustain_stage(0);

        env.tick(true, true, 1.0); // 0.2
        let mid = env.tick(false, true, 1.0); // 0.4
        assert_within!(0.4, mid, 1e-6);

        // Re-trigger: should ramp up from 0.4, not restart at 0.
        let v = env.tick(true, true, 1.0);
        assert_eq!(env.phase_state(), EnvPhase::Active);
        assert!(v > mid, "retrigger should continue upward from {mid}, got {v}");
        assert_within!(0.52, v, 1e-6); // 0.4 + (1.0-0.4)*0.2
    }

    /// A zero-time stage jumps instantly to its target within one tick.
    #[test]
    fn zero_time_stage_is_instant_jump() {
        // Stage 0: instant jump to 1.0. Stage 1 (sustain): instant to 0.5.
        let mut env = EnvCore::new(10.0);
        env.set_stages(&[lin(1.0, 0.0), lin(0.5, 0.0)]);
        env.set_sustain_stage(1);

        let v = env.tick(true, true, 1.0);
        // Both zero-time stages collapse in one tick; held at sustain = 0.5.
        assert_within!(0.5, v, 1e-6);
        assert_eq!(env.phase_state(), EnvPhase::Sustain);
    }

    /// Empty stage list is a no-op that never panics.
    #[test]
    fn empty_stage_list_is_silent() {
        let mut env = EnvCore::new(10.0);
        env.set_stages(&[]);
        for _ in 0..10 {
            let v = env.tick(true, true, 1.0);
            assert_eq!(v, 0.0);
            assert_eq!(env.phase_state(), EnvPhase::Idle);
        }
    }

    /// Velocity (latched level scale) scales stage targets, and the scale is
    /// latched at trigger time.
    #[test]
    fn level_scale_scales_targets() {
        let mut env = EnvCore::new(10.0);
        env.set_stages(&[lin(1.0, 0.5)]);
        env.set_sustain_stage(0);
        env.set_level_scale(0.5);

        env.tick(true, true, 1.0);
        for _ in 0..20 {
            env.tick(false, true, 1.0);
        }
        assert_eq!(env.phase_state(), EnvPhase::Sustain);
        assert_within!(0.5, env.level, 1e-6, "target should scale to 0.5");

        // Changing the scale mid-flight does not affect the in-flight envelope.
        env.set_level_scale(1.0);
        let v = env.tick(false, true, 1.0);
        assert_within!(0.5, v, 1e-6, "latched scale should persist until retrigger");
    }

    /// Gate-off mid pre-sustain ramp jumps straight into the release tail.
    #[test]
    fn gate_off_mid_attack_enters_release() {
        let mut env = EnvCore::new(10.0);
        env.set_stages(&[lin(1.0, 0.5), lin(0.0, 0.5)]);
        env.set_sustain_stage(0);

        env.tick(true, true, 1.0); // 0.2
        let mid = env.tick(false, true, 1.0); // 0.4, still in attack
        assert_eq!(env.phase_state(), EnvPhase::Active);

        // Drop the gate before reaching sustain: release starts from 0.4.
        let r = env.tick(false, false, 1.0);
        assert!(r < mid, "release should descend from {mid}, got {r}");
        for _ in 0..20 {
            env.tick(false, false, 1.0);
        }
        assert_eq!(env.phase_state(), EnvPhase::Idle);
        assert_within!(0.0, env.level, 1e-6);
    }

    /// Exponential curve is monotone and curved (front-loaded), not constant
    /// slope.
    #[test]
    fn exponential_stage_is_curved() {
        let mut env = EnvCore::new(1000.0);
        env.set_stages(&[Stage::new(1.0, 0.1, AdsrShape::Exponential)]);
        env.set_sustain_stage(0);

        let v0 = env.tick(true, true, 1.0);
        let v1 = env.tick(false, true, 1.0);
        let early = v1 - v0;
        // Step forward, then re-measure slope.
        for _ in 0..40 {
            env.tick(false, true, 1.0);
        }
        let a = env.level;
        let b = env.tick(false, true, 1.0);
        let late = b - a;
        assert!(early > 0.0 && late > 0.0, "should be monotone");
        assert!(early > late, "exp slope should flatten: early={early}, late={late}");
    }

    /// `reset()` produces bit-identical output on repeated runs.
    #[test]
    fn reset_is_deterministic() {
        let inputs: [(bool, bool); 8] = [
            (true, true),
            (false, true),
            (false, true),
            (false, true),
            (false, false),
            (false, false),
            (true, true),
            (false, true),
        ];
        let make = || {
            let mut env = EnvCore::new(10.0);
            env.set_stages(&[lin(1.0, 0.3), lin(0.0, 0.3)]);
            env.set_sustain_stage(0);
            env
        };
        assert_reset_deterministic!(
            make(),
            &inputs,
            |env: &mut EnvCore, (t, g): (bool, bool)| env.tick(t, g, 1.0),
            |env: &mut EnvCore| env.reset()
        );
    }
}
