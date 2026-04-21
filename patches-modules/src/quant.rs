use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoInput, MonoOutput, ModuleShape, OutputPort,
};
use patches_core::param_frame::ParamView;
use crate::quant_util::{parse_pitches, quantise_note};

/// Mono V/OCT quantiser.
///
/// Snaps a continuous V/OCT signal to the nearest pitch in a user-supplied
/// set. The set is declared via `channels` (an alias list or count) and one
/// `pitch[i]` parameter per channel. Each pitch is a v/oct value reduced
/// modulo 1.0 into `[0.0, 1.0)`, giving an octave-invariant pitch class.
/// The quantiser is not restricted to 12-tone equal temperament: any
/// microtonal or non-Western scale can be declared by supplying the desired
/// v/oct fractions directly.
///
/// The input is transformed as `centre + in * scale` before quantisation.
/// Emits a one-sample pulse on `trig_out` whenever the quantised pitch
/// changes.
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
/// | `pitch[i]` | float (v/oct) | -8.0--8.0 | `0.0` | Target pitch per scale degree (i in 0..N-1, N = channels) |
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
        let n = shape.channels.max(1);
        ModuleDescriptor::new("Quant", shape.clone())
            .mono_in("in")
            .mono_out("out")
            .mono_out("trig_out")
            .float_param_multi("pitch", n, -8.0, 8.0, 0.0)
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

    fn update_validated_parameters(&mut self, params: &ParamView<'_>) {
        let channels = self.descriptor.shape.channels.max(1);
        parse_pitches(params, channels, &mut self.notes_buf, &mut self.notes_len);
        let v = params.float("centre");
        self.centre = v;
        let v = params.float("scale");
        self.scale = v;
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
        let voct_frac = x - octave;

        let (nearest, octave_adj) = quantise_note(voct_frac, &self.notes_buf[..self.notes_len]);
        let new_quant = octave + octave_adj as f32 + nearest;

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
    use patches_core::test_support::ModuleHarness;
    use patches_core::parameter_map::{ParameterMap, ParameterValue};
    use patches_core::ModuleShape;

    fn shape(n: usize) -> ModuleShape {
        ModuleShape { channels: n, length: 0, ..Default::default() }
    }

    fn pitch_map(pitches: &[f32]) -> ParameterMap {
        let mut map = ParameterMap::new();
        for (i, &p) in pitches.iter().enumerate() {
            map.insert_param("pitch".to_string(), i, ParameterValue::Float(p));
        }
        map
    }

    fn make_quant(pitches: &[f32]) -> ModuleHarness {
        let mut h = ModuleHarness::build_with_shape::<Quant>(&[], shape(pitches.len()));
        h.update_params_map(&pitch_map(pitches));
        h
    }

    #[test]
    fn snaps_to_root_with_default_notes() {
        let mut h = ModuleHarness::build_with_shape::<Quant>(&[], shape(1));
        h.set_mono("in", 0.5);
        h.tick();
        let out = h.read_mono("out");
        assert!(out == 0.0 || out == 1.0, "expected 0.0 or 1.0, got {out}");
    }

    #[test]
    fn snaps_to_nearest_of_root_and_fifth() {
        // C0 = 0.0 v/oct (root), G0 = 7/12 v/oct (fifth).
        let mut h = make_quant(&[0.0, 7.0 / 12.0]);
        h.set_mono("in", 0.0);
        h.tick();
        assert!((h.read_mono("out") - 0.0).abs() < 1e-5, "got {}", h.read_mono("out"));

        h.set_mono("in", 7.0 / 12.0);
        h.tick();
        assert!((h.read_mono("out") - 7.0 / 12.0).abs() < 1e-5);
    }

    #[test]
    fn trig_out_fires_on_change_then_clears() {
        let mut h = make_quant(&[0.0, 7.0 / 12.0]);
        h.set_mono("in", 0.0);
        h.tick();
        assert_eq!(h.read_mono("trig_out"), 0.0);

        h.set_mono("in", 7.0 / 12.0);
        h.tick();
        assert_eq!(h.read_mono("trig_out"), 1.0);

        h.tick();
        assert_eq!(h.read_mono("trig_out"), 0.0);
    }

    #[test]
    fn centre_and_scale_applied() {
        let mut h = ModuleHarness::build_with_shape::<Quant>(
            &[
                ("centre", ParameterValue::Float(1.0)),
                ("scale", ParameterValue::Float(0.5)),
            ],
            shape(1),
        );
        h.set_mono("in", 0.0);
        h.tick();
        assert!((h.read_mono("out") - 1.0).abs() < 1e-5);
    }

    #[test]
    fn pitches_reduced_modulo_octave() {
        // C0 and C1 both reduce to class 0 → single-note scale.
        let mut h = make_quant(&[0.0, 1.0]);
        h.set_mono("in", 0.3);
        h.tick();
        // Should snap to nearest C (either 0.0 or 1.0).
        let out = h.read_mono("out");
        assert!(out == 0.0 || out == 1.0, "got {out}");
    }
}
