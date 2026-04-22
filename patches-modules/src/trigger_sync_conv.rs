//! Explicit converters between ADR 0030 0/1 trigger pulses (on `Mono` cables)
//! and ADR 0047 sub-sample sync events (on `Trigger` cables).
//!
//! These exist because the two conventions share a common buffer layout but
//! use different value encodings, and the graph validator forbids implicit
//! coercion between them. Use `TriggerToSync` to feed a sync port from a
//! sequencer / drum output; use `SyncToTrigger` to feed an envelope or S&H
//! from an oscillator's `reset_out`.

use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    ModuleShape, MonoInput, MonoOutput, OutputPort, TRIGGER_THRESHOLD,
};
use patches_core::param_frame::ParamView;

/// Sample-accurate 0/1 pulse → sub-sample sync event.
///
/// Detects a rising edge across the 0.5 threshold and emits an event at
/// fractional position `1.0` (the sample boundary) on the output `Trigger`
/// cable. All other samples emit `0.0`. Sub-sample precision is unavailable
/// from a sample-accurate source, so events snap to the sample boundary.
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `in` | mono | Sample-accurate 0/1 pulse |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out` | trigger | Sub-sample sync event (`1.0` on rising edge, `0.0` otherwise) |
pub struct TriggerToSync {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    input: MonoInput,
    output: MonoOutput,
    prev: f32,
}

impl Module for TriggerToSync {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("TriggerToSync", shape.clone())
            .mono_in("in")
            .trigger_out("out")
    }

    fn prepare(_env: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        Self {
            instance_id,
            descriptor,
            input: MonoInput::default(),
            output: MonoOutput::default(),
            prev: 0.0,
        }
    }

    fn update_validated_parameters(&mut self, _params: &ParamView<'_>) {}

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.input = MonoInput::from_ports(inputs, 0);
        self.output = outputs[0].expect_trigger();
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let v = pool.read_mono(&self.input);
        let rose = v >= TRIGGER_THRESHOLD && self.prev < TRIGGER_THRESHOLD;
        self.prev = v;
        pool.write_mono(&self.output, if rose { 1.0 } else { 0.0 });
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

/// Sub-sample sync event → sample-accurate 0/1 pulse.
///
/// Emits `1.0` on samples where the input has an event (any value `> 0.0`)
/// and `0.0` otherwise. Fractional position is discarded. Useful for
/// feeding envelopes / S&H modules from an oscillator's `reset_out`.
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `in` | trigger | Sub-sample sync source |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out` | mono | `1.0` on event samples, `0.0` otherwise |
pub struct SyncToTrigger {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    input: MonoInput,
    output: MonoOutput,
}

impl Module for SyncToTrigger {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("SyncToTrigger", shape.clone())
            .trigger_in("in")
            .mono_out("out")
    }

    fn prepare(_env: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        Self {
            instance_id,
            descriptor,
            input: MonoInput::default(),
            output: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, _params: &ParamView<'_>) {}

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.input = inputs[0].expect_trigger();
        self.output = MonoOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let v = pool.read_mono(&self.input);
        pool.write_mono(&self.output, if v > 0.0 { 1.0 } else { 0.0 });
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::test_support::ModuleHarness;

    fn env() -> AudioEnvironment {
        AudioEnvironment { sample_rate: 44100.0, poly_voices: 16, periodic_update_interval: 32, hosted: false }
    }

    #[test]
    fn trigger_to_sync_emits_one_on_rising_edge() {
        let mut h = ModuleHarness::build_with_env::<TriggerToSync>(&[], env());
        // prev = 0, in = 0 → no edge
        h.set_mono("in", 0.0);
        h.tick();
        assert_eq!(h.read_mono("out"), 0.0);
        // rising edge 0 → 1
        h.set_mono("in", 1.0);
        h.tick();
        assert_eq!(h.read_mono("out"), 1.0);
        // held high — no retrigger
        h.tick();
        assert_eq!(h.read_mono("out"), 0.0);
        // falling edge — no event
        h.set_mono("in", 0.0);
        h.tick();
        assert_eq!(h.read_mono("out"), 0.0);
    }

    #[test]
    fn sync_to_trigger_emits_one_on_any_event() {
        let mut h = ModuleHarness::build_with_env::<SyncToTrigger>(&[], env());
        h.set_mono("in", 0.0);
        h.tick();
        assert_eq!(h.read_mono("out"), 0.0);
        h.set_mono("in", 0.3);
        h.tick();
        assert_eq!(h.read_mono("out"), 1.0);
        h.set_mono("in", 1.0);
        h.tick();
        assert_eq!(h.read_mono("out"), 1.0);
        h.set_mono("in", 0.0);
        h.tick();
        assert_eq!(h.read_mono("out"), 0.0);
    }
}
