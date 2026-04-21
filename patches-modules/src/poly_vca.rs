use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    ModuleShape, OutputPort, PolyInput, PolyOutput,
};
use patches_core::param_frame::ParamView;

/// Polyphonic voltage-controlled amplifier.
///
/// Multiplies each voice's signal by its corresponding CV channel.
/// No clamping is applied; negative CV inverts phase.
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `in` | poly | Signal input |
/// | `cv` | poly | Control voltage (per-voice multiplier) |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out` | poly | `in[v] * cv[v]` for each voice |
pub struct PolyVca {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    in_signal: PolyInput,
    in_cv: PolyInput,
    out_audio: PolyOutput,
}

impl Module for PolyVca {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("PolyVca", shape.clone())
            .poly_in("in")
            .poly_in("cv")
            .poly_out("out")
    }

    fn prepare(_audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        Self {
            instance_id,
            descriptor,
            in_signal: PolyInput::default(),
            in_cv: PolyInput::default(),
            out_audio: PolyOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, _params: &ParamView<'_>) {}

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_signal = PolyInput::from_ports(inputs, 0);
        self.in_cv     = PolyInput::from_ports(inputs, 1);
        self.out_audio = PolyOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let signal = pool.read_poly(&self.in_signal);
        let cv     = pool.read_poly(&self.in_cv);
        let mut out = [0.0f32; 16];
        for i in 0..16 {
            out[i] = signal[i] * cv[i];
        }
        pool.write_poly(&self.out_audio, out);
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::test_support::{assert_within, ModuleHarness};

    #[test]
    fn multiplies_per_voice() {
        let mut h = ModuleHarness::build::<PolyVca>(&[]);
        let mut sig = [0.0f32; 16];
        let mut cv  = [0.0f32; 16];
        sig[0] = 0.5;  cv[0] = 0.8;
        sig[1] = 1.0;  cv[1] = 0.0;
        sig[2] = -0.5; cv[2] = 1.0;
        h.set_poly("in", sig);
        h.set_poly("cv", cv);
        h.tick();
        let out = h.read_poly("out");
        assert_within!(0.4, out[0], f32::EPSILON);
        assert_eq!(out[1], 0.0,                          "voice 1: 1.0×0.0=0.0");
        assert_within!(-0.5, out[2], f32::EPSILON);
    }
}
