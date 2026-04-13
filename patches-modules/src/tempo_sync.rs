//! Pure-calculator module: BPM + beat subdivision to milliseconds.
//!
//! Takes a BPM input and a beat subdivision parameter and emits the
//! corresponding tick interval in milliseconds.  Stateless: output is a
//! pure function of inputs each sample.
//!
//! # Inputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `bpm` | mono | Tempo in beats per minute |
//!
//! # Outputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `ms` | mono | Tick interval in milliseconds for the chosen subdivision |
//!
//! # Parameters
//!
//! | Name | Type | Range | Default | Description |
//! |------|------|-------|---------|-------------|
//! | `subdivision` | enum | see below | `1/4` | Beat subdivision selector |
//!
//! Subdivision values: `1/1`, `1/2`, `1/2d`, `1/2t`, `1/4`, `1/4d`, `1/4t`,
//! `1/8`, `1/8d`, `1/8t`, `1/16`, `1/16d`, `1/16t`.
//! Dotted variants multiply the base duration by 1.5; triplet variants
//! multiply by 2/3.

use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoInput, MonoOutput, ModuleShape, OutputPort,
};
use patches_core::parameter_map::{ParameterMap, ParameterValue};

/// BPM + subdivision to ms calculator.  See [module-level documentation](self).
pub struct TempoSync {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    subdivision_factor: f32,
    in_bpm: MonoInput,
    out_ms: MonoOutput,
}

/// Subdivision factor relative to a whole note (1/1 = 1.0).
fn subdivision_factor(name: &str) -> f32 {
    match name {
        "1/1"   => 1.0,
        "1/2"   => 0.5,
        "1/2d"  => 0.75,
        "1/2t"  => 1.0 / 3.0,
        "1/4"   => 0.25,
        "1/4d"  => 0.375,
        "1/4t"  => 1.0 / 6.0,
        "1/8"   => 0.125,
        "1/8d"  => 0.1875,
        "1/8t"  => 1.0 / 12.0,
        "1/16"  => 0.0625,
        "1/16d" => 0.09375,
        "1/16t" => 1.0 / 24.0,
        _       => 0.25,
    }
}

impl Module for TempoSync {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("TempoSync", shape.clone())
            .mono_in("bpm")
            .mono_out("ms")
            .enum_param(
                "subdivision",
                &[
                    "1/1", "1/2", "1/2d", "1/2t",
                    "1/4", "1/4d", "1/4t",
                    "1/8", "1/8d", "1/8t",
                    "1/16", "1/16d", "1/16t",
                ],
                "1/4",
            )
    }

    fn prepare(
        _audio_environment: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
    ) -> Self {
        Self {
            instance_id,
            descriptor,
            subdivision_factor: subdivision_factor("1/4"),
            in_bpm: MonoInput::default(),
            out_ms: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, params: &mut ParameterMap) {
        if let Some(ParameterValue::Enum(v)) = params.get_scalar("subdivision") {
            self.subdivision_factor = subdivision_factor(v);
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_bpm = MonoInput::from_ports(inputs, 0);
        self.out_ms = MonoOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let bpm = pool.read_mono(&self.in_bpm).max(1.0);
        // Whole-note duration in ms = 4 beats * (60_000 ms / bpm)
        let whole_note_ms = 240_000.0 / bpm;
        let ms = whole_note_ms * self.subdivision_factor;
        pool.write_mono(&self.out_ms, ms);
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::test_support::ModuleHarness;
    use patches_core::parameter_map::ParameterValue;

    fn make_tempo_sync(subdivision: &'static str) -> ModuleHarness {
        let mut h = ModuleHarness::build::<TempoSync>(&[
            ("subdivision", ParameterValue::Enum(subdivision)),
        ]);
        h
    }

    #[test]
    fn quarter_note_at_120_bpm() {
        let mut h = make_tempo_sync("1/4");
        h.set_mono("bpm", 120.0);
        h.tick();
        let ms = h.read_mono("ms");
        assert!((ms - 500.0).abs() < 1e-3, "expected 500.0, got {ms}");
    }

    #[test]
    fn eighth_note_at_120_bpm() {
        let mut h = make_tempo_sync("1/8");
        h.set_mono("bpm", 120.0);
        h.tick();
        let ms = h.read_mono("ms");
        assert!((ms - 250.0).abs() < 1e-3, "expected 250.0, got {ms}");
    }

    #[test]
    fn dotted_quarter_at_120_bpm() {
        let mut h = make_tempo_sync("1/4d");
        h.set_mono("bpm", 120.0);
        h.tick();
        let ms = h.read_mono("ms");
        assert!((ms - 750.0).abs() < 1e-3, "expected 750.0, got {ms}");
    }

    #[test]
    fn triplet_eighth_at_120_bpm() {
        let mut h = make_tempo_sync("1/8t");
        h.set_mono("bpm", 120.0);
        h.tick();
        let ms = h.read_mono("ms");
        let expected = 240_000.0 / 120.0 / 12.0;
        assert!((ms - expected).abs() < 1e-2, "expected {expected}, got {ms}");
    }

    #[test]
    fn whole_note_at_60_bpm() {
        let mut h = make_tempo_sync("1/1");
        h.set_mono("bpm", 60.0);
        h.tick();
        let ms = h.read_mono("ms");
        assert!((ms - 4000.0).abs() < 1e-3, "expected 4000.0, got {ms}");
    }

    #[test]
    fn bpm_clamped_to_minimum() {
        let mut h = make_tempo_sync("1/4");
        h.set_mono("bpm", 0.0);
        h.tick();
        let ms = h.read_mono("ms");
        assert!((ms - 60_000.0).abs() < 1e-1, "expected 60000.0, got {ms}");
    }

    #[test]
    fn sixteenth_note_at_140_bpm() {
        let mut h = make_tempo_sync("1/16");
        h.set_mono("bpm", 140.0);
        h.tick();
        let ms = h.read_mono("ms");
        let expected = 240_000.0 / 140.0 * 0.0625;
        assert!((ms - expected).abs() < 1e-2, "expected {expected}, got {ms}");
    }
}
