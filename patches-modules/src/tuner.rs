use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoInput, MonoOutput, ModuleShape, OutputPort,
};
use patches_core::parameter_map::{ParameterMap, ParameterValue};

/// Offsets a V/OCT pitch signal by a fixed interval.
///
/// Output = input + octave + semitones/12 + cents/1200.
/// All three parameters are independent and additive. Setting all to zero
/// passes the signal through unchanged.
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
/// | `out` | mono | Offset V/OCT pitch |
///
/// # Parameters
///
/// | Name | Type | Range | Default | Description |
/// |------|------|-------|---------|-------------|
/// | `octave` | int | -8--8 | `0` | Octave offset |
/// | `semi` | int | -12--12 | `0` | Semitone offset |
/// | `cent` | int | -100--100 | `0` | Cent offset |
pub struct Tuner {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    octave: i64,
    semi: i64,
    cent: i64,
    /// Precomputed offset in V/OCT: octave + semi/12 + cent/1200.
    offset: f32,
    // Port fields
    in_port: MonoInput,
    out_port: MonoOutput,
}

impl Tuner {
    fn recompute_offset(octave: i64, semi: i64, cent: i64) -> f32 {
        octave as f32 + semi as f32 / 12.0 + cent as f32 / 1200.0
    }
}

impl Module for Tuner {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("Tuner", shape.clone())
            .mono_in("in")
            .mono_out("out")
            .int_param("octave", -8,   8,   0)
            .int_param("semi",   -12,  12,  0)
            .int_param("cent",   -100, 100, 0)
    }

    fn prepare(_audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        Self {
            instance_id,
            descriptor,
            octave: 0,
            semi: 0,
            cent: 0,
            offset: 0.0,
            in_port: MonoInput::default(),
            out_port: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, params: &mut ParameterMap) {
        if let Some(ParameterValue::Int(v)) = params.get_scalar("octave") { self.octave = *v; }
        if let Some(ParameterValue::Int(v)) = params.get_scalar("semi")   { self.semi = *v; }
        if let Some(ParameterValue::Int(v)) = params.get_scalar("cent")   { self.cent = *v; }
        self.offset = Self::recompute_offset(self.octave, self.semi, self.cent);
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_port = MonoInput::from_ports(inputs, 0);
        self.out_port = MonoOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let input = pool.read_mono(&self.in_port);
        pool.write_mono(&self.out_port, input + self.offset);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::test_support::{assert_within, ModuleHarness, params};

    fn make_tuner(octave: i64, semi: i64, cent: i64) -> ModuleHarness {
        ModuleHarness::build::<Tuner>(params!["octave" => octave, "semi" => semi, "cent" => cent])
    }

    #[test]
    fn zero_offsets_pass_through() {
        let mut h = make_tuner(0, 0, 0);
        h.set_mono("in", 3.0);
        h.tick();
        assert_within!(3.0, h.read_mono("out"), 1e-12_f32);
    }

    #[test]
    fn octave_offset_adds_integer() {
        let mut h = make_tuner(1, 0, 0);
        h.set_mono("in", 4.0);
        h.tick();
        assert_within!(5.0, h.read_mono("out"), 1e-12_f32);
    }

    #[test]
    fn semitone_offset_adds_one_twelfth() {
        let mut h = make_tuner(0, 1, 0);
        h.set_mono("in", 4.0);
        h.tick();
        assert_within!(4.0 + 1.0 / 12.0, h.read_mono("out"), 1e-12_f32);
    }

    #[test]
    fn cent_offset_adds_one_twelfth_hundredth() {
        let mut h = make_tuner(0, 0, 100);
        h.set_mono("in", 4.0);
        h.tick();
        assert_within!(4.0 + 100.0 / 1200.0, h.read_mono("out"), 1e-12_f32);
    }

    #[test]
    fn combined_offsets_are_additive() {
        let mut h = make_tuner(-1, 3, -50);
        h.set_mono("in", 4.0);
        h.tick();
        let expected = 4.0 - 1.0 + 3.0 / 12.0 - 50.0 / 1200.0;
        assert_within!(expected, h.read_mono("out"), 1e-12_f32);
    }

    #[test]
    fn partial_update_preserves_unchanged_params() {
        // Simulates the planner sending only the changed key on hot-reload.
        let mut h = make_tuner(1, 7, 12);
        h.update_validated_parameters(params!["cent" => 0_i64]);
        h.set_mono("in", 4.0);
        h.tick();
        // octave=1, semi=7, cent=0 — octave and semi must be retained from initial build.
        let expected = 4.0 + 1.0 + 7.0 / 12.0;
        assert_within!(expected, h.read_mono("out"), 1e-5_f32);
    }
}
