use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    ModuleShape, OutputPort, PolyInput, PolyOutput,
};
use patches_core::param_frame::ParamView;

/// Polyphonic V/OCT pitch offset: applies the same fixed interval to all voices.
///
/// Output[v] = input[v] + octave + semitones/12 + cents/1200.
/// All three parameters are independent and additive. Setting all to zero
/// passes the signal through unchanged.
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
/// | `out` | poly | Offset V/OCT pitch (per-voice) |
///
/// # Parameters
///
/// | Name | Type | Range | Default | Description |
/// |------|------|-------|---------|-------------|
/// | `octave` | int | -8--8 | `0` | Octave offset |
/// | `semi` | int | -12--12 | `0` | Semitone offset |
/// | `cent` | int | -100--100 | `0` | Cent offset |
pub struct PolyTuner {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    octave: i64,
    semi: i64,
    cent: i64,
    /// Precomputed offset in V/OCT: octave + semi/12 + cent/1200.
    offset: f32,
    in_port: PolyInput,
    out_port: PolyOutput,
}

impl PolyTuner {
    fn recompute_offset(octave: i64, semi: i64, cent: i64) -> f32 {
        octave as f32 + semi as f32 / 12.0 + cent as f32 / 1200.0
    }
}

impl Module for PolyTuner {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("PolyTuner", shape.clone())
            .poly_in("in")
            .poly_out("out")
            .int_param("octave", -8,   8,   0)
            .int_param("semi",   -12,  12,  0)
            .int_param("cent",   -100, 100, 0)
    }

    fn prepare(_env: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        Self {
            instance_id,
            descriptor,
            octave: 0,
            semi: 0,
            cent: 0,
            offset: 0.0,
            in_port: PolyInput::default(),
            out_port: PolyOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, params: &ParamView<'_>) {
        self.octave = params.int("octave");
        self.semi = params.int("semi");
        self.cent = params.int("cent");
        self.offset = Self::recompute_offset(self.octave, self.semi, self.cent);
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_port  = PolyInput::from_ports(inputs, 0);
        self.out_port = PolyOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let mut voices = pool.read_poly(&self.in_port);
        for v in &mut voices {
            *v += self.offset;
        }
        pool.write_poly(&self.out_port, voices);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::test_support::{assert_within, ModuleHarness, params};

    fn make(octave: i64, semi: i64, cent: i64) -> ModuleHarness {
        ModuleHarness::build::<PolyTuner>(params!["octave" => octave, "semi" => semi, "cent" => cent])
    }

    #[test]
    fn zero_offsets_pass_through() {
        let mut h = make(0, 0, 0);
        let voices: [f32; 16] = std::array::from_fn(|i| i as f32 * 0.25);
        h.set_poly("in", voices);
        h.tick();
        let out = h.read_poly("out");
        for (&a, &b) in voices.iter().zip(out.iter()) {
            assert_within!(a, b, 1e-12_f32);
        }
    }

    #[test]
    fn octave_offset_applied_to_all_voices() {
        let mut h = make(1, 0, 0);
        let voices: [f32; 16] = std::array::from_fn(|i| i as f32 * 0.1);
        h.set_poly("in", voices);
        h.tick();
        let out = h.read_poly("out");
        for (&a, &b) in voices.iter().zip(out.iter()) {
            assert_within!(a + 1.0, b, 1e-12_f32);
        }
    }

    #[test]
    fn semitone_offset_applied_to_all_voices() {
        let mut h = make(0, 1, 0);
        let voices: [f32; 16] = [2.0; 16];
        h.set_poly("in", voices);
        h.tick();
        let out = h.read_poly("out");
        let expected = 2.0 + 1.0 / 12.0;
        for &b in out.iter() {
            assert_within!(expected, b, 1e-12_f32);
        }
    }

    #[test]
    fn combined_offsets_are_additive() {
        let mut h = make(-1, 3, -50);
        let voices: [f32; 16] = [4.0; 16];
        h.set_poly("in", voices);
        h.tick();
        let out = h.read_poly("out");
        let expected = 4.0 - 1.0 + 3.0 / 12.0 - 50.0 / 1200.0;
        for &b in out.iter() {
            assert_within!(expected, b, 1e-5_f32);
        }
    }
}
