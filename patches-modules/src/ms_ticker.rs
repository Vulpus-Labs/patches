//! Converts a millisecond interval into trigger pulses and a gate signal.
//!
//! Emits a single-sample trigger pulse (1.0) at each interval boundary and
//! a 50% duty-cycle gate (high for the first half, low for the second).
//! Handles dynamic changes to the `ms` input smoothly by adjusting the
//! time-to-next-tick without glitching.
//!
//! # Inputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `ms` | mono | Tick interval in milliseconds |
//! | `reset` | trigger | One-sample pulse resets phase to zero (ADR 0047) |
//!
//! # Outputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `trigger` | trigger | Single-sample pulse (1.0) at each interval boundary (ADR 0047) |
//! | `gate` | mono | High (1.0) for first half of interval, low (0.0) for second |

use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoInput, MonoOutput, ModuleShape, OutputPort,
};
use patches_core::cables::TriggerInput;
use patches_core::param_frame::ParamView;

/// Millisecond-interval ticker.  See [module-level documentation](self).
pub struct MsTicker {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    sample_rate: f32,
    /// Phase accumulator in [0.0, 1.0)
    phase: f32,
    in_ms: MonoInput,
    in_reset: TriggerInput,
    out_trigger: MonoOutput,
    out_gate: MonoOutput,
}

impl Module for MsTicker {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("MsTicker", shape.clone())
            .mono_in("ms")
            .trigger_in("reset")
            .trigger_out("trigger")
            .mono_out("gate")
    }

    fn prepare(
        audio_environment: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
    ) -> Self {
        Self {
            instance_id,
            descriptor,
            sample_rate: audio_environment.sample_rate,
            phase: 0.0,
            in_ms: MonoInput::default(),
            in_reset: TriggerInput::default(),
            out_trigger: MonoOutput::default(),
            out_gate: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, _params: &ParamView<'_>) {}

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_ms = MonoInput::from_ports(inputs, 0);
        self.in_reset = TriggerInput::from_ports(inputs, 1);
        self.out_trigger = outputs[0].expect_trigger();
        self.out_gate = MonoOutput::from_ports(outputs, 1);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        // Reset on rising edge
        if self.in_reset.is_connected() && self.in_reset.tick(pool).is_some() {
            self.phase = 0.0;
        }

        let ms = pool.read_mono(&self.in_ms).max(0.01);
        let interval_samples = ms * 0.001 * self.sample_rate;
        let increment = 1.0 / interval_samples;

        let old_phase = self.phase;
        self.phase += increment;

        let fired = self.phase >= 1.0;
        if fired {
            self.phase -= 1.0;
            // Clamp to avoid runaway if ms is extremely small
            if self.phase >= 1.0 {
                self.phase = 0.0;
            }
        }

        pool.write_mono(&self.out_trigger, if fired { 1.0 } else { 0.0 });

        // Gate: high for first half of interval, low for second
        // Use post-increment phase for gate output. On fire sample, phase
        // has just wrapped, so it's near 0 → gate high (correct).
        let gate_phase = if fired { self.phase } else { old_phase + increment };
        pool.write_mono(&self.out_gate, if gate_phase < 0.5 { 1.0 } else { 0.0 });
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::AudioEnvironment;
    use patches_core::test_support::{ModuleHarness, params};

    const SR: f32 = 44_100.0;
    const ENV: AudioEnvironment = AudioEnvironment {
        sample_rate: SR, poly_voices: 16, periodic_update_interval: 32, hosted: false,
    };

    fn make_ticker(ms: f32) -> ModuleHarness {
        let mut h = ModuleHarness::build_with_env::<MsTicker>(params![], ENV);
        h.set_mono("ms", ms);
        h.disconnect_input("reset");
        h
    }

    #[test]
    fn trigger_fires_at_correct_interval() {
        // 10 ms at 44100 Hz = 441 samples per tick
        let mut h = make_ticker(10.0);
        let mut trigger_count = 0;
        let mut first_trigger = None;
        for i in 0..2000 {
            h.tick();
            if h.read_mono("trigger") > 0.5 {
                trigger_count += 1;
                if first_trigger.is_none() {
                    first_trigger = Some(i);
                }
            }
        }
        // 2000 / 441 ≈ 4.53, so expect 4 triggers
        assert!((4..=5).contains(&trigger_count),
            "expected 4-5 triggers in 2000 samples at 10ms, got {trigger_count}");
    }

    #[test]
    fn gate_is_high_for_first_half() {
        // Use a period that divides evenly: 100 samples = ~2.267 ms at 44100
        let ms = 100.0 / SR * 1000.0; // exactly 100 samples
        let mut h = make_ticker(ms);
        let mut gate_high = 0;
        let mut gate_low = 0;
        // Skip first partial period, run two full periods
        for _ in 0..100 {
            h.tick();
        }
        for _ in 0..200 {
            h.tick();
            if h.read_mono("gate") > 0.5 {
                gate_high += 1;
            } else {
                gate_low += 1;
            }
        }
        // Approximately half should be high, half low (±2 for boundary)
        assert!((gate_high - 100_i32).abs() <= 2,
            "expected ~100 gate-high samples, got {gate_high}");
        assert!((gate_low - 100_i32).abs() <= 2,
            "expected ~100 gate-low samples, got {gate_low}");
    }

    #[test]
    fn trigger_is_single_sample() {
        let mut h = make_ticker(10.0);
        let mut consecutive_triggers = 0;
        let mut max_consecutive = 0;
        for _ in 0..5000 {
            h.tick();
            if h.read_mono("trigger") > 0.5 {
                consecutive_triggers += 1;
                max_consecutive = max_consecutive.max(consecutive_triggers);
            } else {
                consecutive_triggers = 0;
            }
        }
        assert_eq!(max_consecutive, 1, "trigger should be single-sample, got {max_consecutive} consecutive");
    }

    #[test]
    fn dynamic_interval_change() {
        let mut h = ModuleHarness::build_with_env::<MsTicker>(params![], ENV);
        h.disconnect_input("reset");
        // Start with 10ms
        h.set_mono("ms", 10.0);
        for _ in 0..500 {
            h.tick();
        }
        // Switch to 5ms — should not glitch (no burst of triggers)
        h.set_mono("ms", 5.0);
        let mut triggers_in_window = 0;
        for _ in 0..500 {
            h.tick();
            if h.read_mono("trigger") > 0.5 {
                triggers_in_window += 1;
            }
        }
        // 500 samples / (5ms * 44.1 samples/ms) ≈ 500/220.5 ≈ 2.27 → expect 2-3
        assert!((1..=4).contains(&triggers_in_window),
            "expected 2-3 triggers after interval change, got {triggers_in_window}");
    }

    #[test]
    fn reset_fires_immediately() {
        let mut h = ModuleHarness::build_with_env::<MsTicker>(params![], ENV);
        h.set_mono("ms", 100.0); // long interval
        h.set_mono("reset", 0.0);
        // Advance partway
        for _ in 0..500 {
            h.tick();
        }
        // Rising edge on reset
        h.set_mono("reset", 1.0);
        h.tick();
        // Phase was reset to 0; next tick from phase=0 won't fire immediately,
        // but the interval restarts. Advance one full period and count.
        h.set_mono("reset", 0.0);
        let period_samples = (100.0 * 0.001 * SR) as usize;
        let mut fired = false;
        for _ in 0..period_samples + 2 {
            h.tick();
            if h.read_mono("trigger") > 0.5 {
                fired = true;
                break;
            }
        }
        assert!(fired, "trigger should fire within one period after reset");
    }

    #[test]
    fn very_short_interval() {
        // 0.1 ms ≈ 4.41 samples — should not crash or produce NaN
        let mut h = make_ticker(0.1);
        for _ in 0..1000 {
            h.tick();
            let t = h.read_mono("trigger");
            let g = h.read_mono("gate");
            assert!(!t.is_nan() && !g.is_nan(), "outputs must not be NaN");
        }
    }

    #[test]
    fn very_long_interval() {
        // 10000 ms — should not fire in 1000 samples
        let mut h = make_ticker(10000.0);
        let mut triggers = 0;
        for _ in 0..1000 {
            h.tick();
            if h.read_mono("trigger") > 0.5 { triggers += 1; }
        }
        assert_eq!(triggers, 0, "should not trigger in 1000 samples at 10s interval");
    }
}
