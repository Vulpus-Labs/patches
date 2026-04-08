use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    ModuleShape, OutputPort, PolyInput, PolyOutput,
};
use patches_core::parameter_map::{ParameterMap, ParameterValue};
use crate::quant_util::{parse_notes, quantise_note};

/// Polyphonic V/OCT quantiser.
///
/// Applies the same quantisation logic as [`Quant`](crate::quant::Quant)
/// independently to each of 16 voices. Always free-running.
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `in` | poly | V/OCT pitch signal (per-voice) |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out` | poly | Quantised V/OCT pitch (per-voice) |
/// | `trig_out` | poly | One-sample pulse on pitch change (per-voice) |
///
/// # Parameters
///
/// | Name | Type | Range | Default | Description |
/// |------|------|-------|---------|-------------|
/// | `notes` | str array | up to 12 entries | `["0"]` | Semitone values in the scale |
/// | `centre` | float | -4.0--4.0 | `0.0` | Offset added before quantisation |
/// | `scale` | float | -4.0--4.0 | `1.0` | Multiplier applied to input before quantisation |
pub struct PolyQuant {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    notes_buf: [f32; 12],
    notes_len: usize,
    last_quantised: [f32; 16],
    pending_trig_out: [f32; 16],
    centre: f32,
    scale: f32,
    in_sig: PolyInput,
    out: PolyOutput,
    trig_out: PolyOutput,
}

impl Module for PolyQuant {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("PolyQuant", shape.clone())
            .poly_in("in")
            .poly_out("out")
            .poly_out("trig_out")
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
            last_quantised: [0.0; 16],
            pending_trig_out: [0.0; 16],
            centre: 0.0,
            scale: 1.0,
            in_sig: PolyInput::default(),
            out: PolyOutput::default(),
            trig_out: PolyOutput::default(),
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
        self.in_sig  = PolyInput::from_ports(inputs, 0);
        self.out     = PolyOutput::from_ports(outputs, 0);
        self.trig_out = PolyOutput::from_ports(outputs, 1);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let voices = pool.read_poly(&self.in_sig);
        let notes = &self.notes_buf[..self.notes_len];

        for (i, &x) in voices.iter().enumerate() {
            let centred_and_scaled = self.centre + x * self.scale;
            let octave = centred_and_scaled.floor();
            let semitone_frac = (centred_and_scaled - octave) * 12.0;
            let (nearest, octave_adj) = quantise_note(semitone_frac, notes);
            let new_quant = octave + octave_adj as f32 + nearest / 12.0;
            if (new_quant - self.last_quantised[i]).abs() > 1e-6 {
                self.pending_trig_out[i] = 1.0;
                self.last_quantised[i] = new_quant;
            }
        }

        let mut out_buf = [0.0f32; 16];
        for (slot, &lq) in out_buf.iter_mut().zip(self.last_quantised.iter()) {
            *slot = lq;
        }

        pool.write_poly(&self.out, out_buf);
        pool.write_poly(&self.trig_out, self.pending_trig_out);
        self.pending_trig_out = [0.0; 16];
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::test_support::ModuleHarness;
    use patches_core::parameter_map::ParameterValue;

    fn make_poly_quant(notes: &[&str]) -> ModuleHarness {
        let arr: Vec<String> = notes.iter().map(|s| s.to_string()).collect();
        ModuleHarness::build::<PolyQuant>(&[("notes", ParameterValue::Array(arr.into()))])
    }

    #[test]
    fn quantises_each_voice_independently() {
        let mut h = make_poly_quant(&["0", "7"]);
        let mut input = [0.0f32; 16];
        input[0] = 0.0;          // → 0.0  (root)
        input[1] = 7.0 / 12.0;  // → 7/12 (fifth)
        h.set_poly("in", input);
        h.tick();
        let out = h.read_poly("out");
        assert!((out[0] - 0.0).abs() < 1e-5, "voice 0 expected 0.0 got {}", out[0]);
        assert!((out[1] - 7.0 / 12.0).abs() < 1e-5, "voice 1 expected 7/12 got {}", out[1]);
    }

    #[test]
    fn per_voice_trig_out_fires_independently() {
        let mut h = make_poly_quant(&["0", "7"]);
        // First tick with all zeros: no change from initial state (last_quantised starts at 0.0)
        h.set_poly("in", [0.0; 16]);
        h.tick();

        // Change voice 1 only
        let mut input = [0.0f32; 16];
        input[1] = 7.0 / 12.0;
        h.set_poly("in", input);
        h.tick();
        let trig = h.read_poly("trig_out");
        assert_eq!(trig[0], 0.0, "voice 0 should not fire");
        assert_eq!(trig[1], 1.0, "voice 1 should fire");
    }

    #[test]
    fn trig_out_clears_next_sample() {
        let mut h = make_poly_quant(&["0", "7"]);
        let mut input = [0.0f32; 16];
        input[0] = 7.0 / 12.0; // triggers a change
        h.set_poly("in", input);
        h.tick();
        h.tick(); // same input, no change
        let trig = h.read_poly("trig_out");
        assert_eq!(trig[0], 0.0, "trig_out must clear after one sample");
    }
}
