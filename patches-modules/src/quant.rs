use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoInput, MonoOutput, ModuleShape, OutputPort,
};
use patches_core::parameter_map::{ParameterMap, ParameterValue};
use crate::quant_util::{parse_notes, quantise_note};

/// Mono V/OCT quantiser.
///
/// Snaps a continuous V/OCT signal to the nearest note in a user-supplied
/// semitone set. The input is transformed as `centre + in * scale` before
/// quantisation. Emits a one-sample pulse on `trig_out` whenever the
/// quantised pitch changes.
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `in` | mono | V/OCT pitch signal |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out` | mono | Quantised V/OCT pitch |
/// | `trig_out` | mono | One-sample pulse on pitch change |
///
/// # Parameters
///
/// | Name | Type | Range | Default | Description |
/// |------|------|-------|---------|-------------|
/// | `notes` | str array | up to 12 entries | `["0"]` | Semitone values in the scale |
/// | `centre` | float | -4.0--4.0 | `0.0` | Offset added before quantisation |
/// | `scale` | float | -4.0--4.0 | `1.0` | Multiplier applied to input before quantisation |
pub struct Quant {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    notes_buf: [f32; 12],
    notes_len: usize,
    last_quantised: f32,
    pending_trig_out: f32,
    centre: f32,
    scale: f32,
    in_sig: MonoInput,
    out: MonoOutput,
    trig_out: MonoOutput,
}

impl Module for Quant {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("Quant", shape.clone())
            .mono_in("in")
            .mono_out("out")
            .mono_out("trig_out")
            .array_param("notes", &["0"], 12)
            .float_param("centre", -4.0, 4.0, 0.0)
            .float_param("scale", -4.0, 4.0, 1.0)
    }

    fn prepare(_env: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        let mut notes_buf = [0.0f32; 12];
        notes_buf[0] = 0.0;
        Self {
            instance_id,
            descriptor,
            notes_buf,
            notes_len: 1,
            last_quantised: 0.0,
            pending_trig_out: 0.0,
            centre: 0.0,
            scale: 1.0,
            in_sig: MonoInput::default(),
            out: MonoOutput::default(),
            trig_out: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, params: &mut ParameterMap) {
        if let Some(ParameterValue::Array(strings)) = params.get_scalar("notes") {
            parse_notes(strings, &mut self.notes_buf, &mut self.notes_len);
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("centre") {
            self.centre = *v;
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("scale") {
            self.scale = *v;
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_sig  = MonoInput::from_ports(inputs, 0);
        self.out     = MonoOutput::from_ports(outputs, 0);
        self.trig_out = MonoOutput::from_ports(outputs, 1);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let x = self.centre + pool.read_mono(&self.in_sig) * self.scale;
        let octave = x.floor();
        let semitone_frac = (x - octave) * 12.0;

        let (nearest, octave_adj) = quantise_note(semitone_frac, &self.notes_buf[..self.notes_len]);
        let new_quant = octave + octave_adj as f32 + nearest / 12.0;

        if (new_quant - self.last_quantised).abs() > 1e-6 {
            self.pending_trig_out = 1.0;
            self.last_quantised = new_quant;
        }

        pool.write_mono(&self.out, self.last_quantised);
        pool.write_mono(&self.trig_out, self.pending_trig_out);
        self.pending_trig_out = 0.0;
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::test_support::{ModuleHarness, params};
    use patches_core::parameter_map::ParameterValue;

    fn make_quant(notes: &[&str]) -> ModuleHarness {
        let arr: Vec<String> = notes.iter().map(|s| s.to_string()).collect();
        ModuleHarness::build::<Quant>(&[("notes", ParameterValue::Array(arr.into()))])
    }

    #[test]
    fn snaps_to_root_with_default_notes() {
        let mut h = ModuleHarness::build::<Quant>(params![]);
        h.set_mono("in", 0.5); // 0.5 voct → halfway between C0 and C1, semitone 6
        h.tick();
        // Only note is 0 (C), so it should snap to C in the nearest octave.
        let out = h.read_mono("out");
        assert!(out == 0.0 || out == 1.0, "expected 0.0 or 1.0, got {out}");
    }

    #[test]
    fn snaps_to_nearest_of_root_and_fifth() {
        let mut h = make_quant(&["0", "7"]);
        // Input 0.0 voct → semitone 0 → exact match root → out = 0.0
        h.set_mono("in", 0.0);
        h.tick();
        assert!((h.read_mono("out") - 0.0).abs() < 1e-5, "expected 0.0 got {}", h.read_mono("out"));

        // Input = 0 + 7/12 → semitone 7 → exact match fifth → out = 7/12
        h.set_mono("in", 7.0 / 12.0);
        h.tick();
        let expected = 7.0 / 12.0;
        assert!((h.read_mono("out") - expected).abs() < 1e-5);
    }

    #[test]
    fn trig_out_fires_on_change_then_clears() {
        let mut h = make_quant(&["0", "7"]);
        h.set_mono("in", 0.0);
        h.tick(); // first quantise: change from initial 0.0 - no change, stays 0.0
        // Actually initial last_quantised is 0.0 and first quantise is 0.0 so no trig
        assert_eq!(h.read_mono("trig_out"), 0.0);

        h.set_mono("in", 7.0 / 12.0);
        h.tick();
        assert_eq!(h.read_mono("trig_out"), 1.0, "trig_out should fire on pitch change");

        h.tick(); // same input
        assert_eq!(h.read_mono("trig_out"), 0.0, "trig_out should clear next sample");
    }

    #[test]
    fn centre_and_scale_applied() {
        let arr = vec!["0".to_string()];
        let mut h = ModuleHarness::build::<Quant>(&[
            ("notes", ParameterValue::Array(arr.into())),
            ("centre", ParameterValue::Float(1.0)),
            ("scale", ParameterValue::Float(0.5)),
        ]);
        h.set_mono("in", 0.0);
        h.tick();
        // quantised_voct = 0.0, out = 1.0 + 0.0 * 0.5 = 1.0
        assert!((h.read_mono("out") - 1.0).abs() < 1e-5);
    }
}
