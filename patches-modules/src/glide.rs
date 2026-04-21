use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoInput, MonoOutput, ModuleShape, OutputPort,
};
use patches_core::param_frame::ParamView;

/// A portamento (pitch glide) module.
///
/// Smooths V/OCT pitch values using a one-pole low-pass filter. Because V/OCT
/// is a log-frequency scale (1 V/OCT = 1 octave), interpolating linearly in
/// V/OCT space gives perceptually linear (constant-ratio) glide.
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
/// | `out` | mono | Smoothed V/OCT pitch |
///
/// # Parameters
///
/// | Name | Type | Range | Default | Description |
/// |------|------|-------|---------|-------------|
/// | `glide_ms` | float | 0.0--10000.0 | `100.0` | Glide time in milliseconds |
pub struct Glide {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    /// Current smoothed V/OCT value (C2 = 0.0).
    voct: f32,
    alpha: f32,
    beta: f32,
    glide_ms: f32,
    sample_rate: f32,
    // Port fields
    in_port: MonoInput,
    out_port: MonoOutput,
}

impl Glide {
    fn update_beta(&mut self) {
        let n_samples = self.sample_rate * self.glide_ms / 1000.0;
        if n_samples <= 0.0 {
            self.beta = 1.0;
        } else {
            self.beta = 1.0 - self.alpha.powf(1.0 / n_samples);
        }
    }

    fn set_glide_ms(&mut self, glide_ms: f32) {
        self.glide_ms = glide_ms;
        self.update_beta();
    }
}

impl Module for Glide {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("Glide", shape.clone())
            .mono_in("in")
            .mono_out("out")
            .float_param("glide_ms", 0.0, 10_000.0, 100.0)
    }

    fn prepare(audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        Self {
            instance_id,
            descriptor,
            voct: 0.0,
            alpha: 0.01,
            beta: 0.0,
            glide_ms: 0.0,
            sample_rate: audio_environment.sample_rate,
            in_port: MonoInput::default(),
            out_port: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, params: &ParamView<'_>) {
        let v = params.float("glide_ms");
        self.set_glide_ms(v);
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
        // Input is V/OCT (C2 = 0.0). Interpolate directly in V/OCT space —
        // no ln/exp needed since V/OCT is already a log-frequency scale.
        let input = pool.read_mono(&self.in_port);
        self.voct += self.beta * (input - self.voct);
        pool.write_mono(&self.out_port, self.voct);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::AudioEnvironment;
    use patches_core::test_support::{assert_within, ModuleHarness, params};

    fn make_glide_harness(glide_ms: f32, sample_rate: f32) -> ModuleHarness {
        ModuleHarness::build_with_env::<Glide>(
            params!["glide_ms" => glide_ms],
            AudioEnvironment { sample_rate, poly_voices: 16, periodic_update_interval: 32, hosted: false },
        )
    }

    #[test]
    fn output_tracks_input_with_glide() {
        let mut h = make_glide_harness(500.0, 44100.0);
        h.set_mono("in", 1.0);
        h.tick();
        let after_start = h.read_mono("out");

        h.set_mono("in", 2.0);
        h.tick();
        let after_step = h.read_mono("out");

        assert!(
            after_step < 2.0,
            "expected output {after_step} to be below target 2.0 (glide should smooth)"
        );
        assert!(
            after_step > after_start,
            "expected output {after_step} to have increased from {after_start}"
        );
    }

    #[test]
    fn zero_glide_ms_tracks_instantly() {
        let mut h = make_glide_harness(0.0, 44100.0);
        h.set_mono("in", 2.0);
        h.tick();
        assert_within!(2.0, h.read_mono("out"), 1e-9_f32);
    }

    #[test]
    fn c2_voct_zero_is_not_held() {
        let mut h = make_glide_harness(0.0, 44100.0);
        // Prime at C3 = 1.0 V/OCT.
        h.set_mono("in", 1.0);
        h.tick();
        assert_within!(1.0, h.read_mono("out"), 1e-9_f32);
        // Now target C2 = 0.0 V/OCT.
        h.set_mono("in", 0.0);
        h.tick();
        assert_within!(
            0.0, h.read_mono("out"), 1e-9_f32
        );
    }
}
