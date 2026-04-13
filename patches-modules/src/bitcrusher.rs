/// Sample-rate and bit-depth reduction effect.
///
/// Combines rate reduction (sample-and-hold with fractional phase accumulator)
/// and bit-depth reduction (uniform quantisation) for classic lo-fi degradation.
/// Both parameters accept CV modulation.
///
/// # Inputs
///
/// | Port       | Kind | Description     |
/// |------------|------|-----------------|
/// | `in`       | mono | Audio input     |
/// | `rate_cv`  | mono | Rate modulation (additive, ±1.0 maps to full range) |
/// | `depth_cv` | mono | Depth modulation (additive, ±1.0 maps to full range) |
///
/// # Outputs
///
/// | Port  | Kind | Description      |
/// |-------|------|------------------|
/// | `out` | mono | Processed output |
///
/// # Parameters
///
/// | Name      | Type  | Range      | Default | Description                          |
/// |-----------|-------|------------|---------|--------------------------------------|
/// | `rate`    | float | 0.0--1.0   | `1.0`   | Rate reduction (1.0 = full rate)     |
/// | `depth`   | float | 1.0--32.0  | `32.0`  | Bit depth (continuous)               |
/// | `dry_wet` | float | 0.0--1.0   | `1.0`   | Dry/wet mix                          |
use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoInput, MonoOutput, ModuleShape, OutputPort, PeriodicUpdate,
};
use patches_core::parameter_map::{ParameterMap, ParameterValue};
use patches_dsp::BitcrusherKernel;

pub struct Bitcrusher {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    sample_rate: f32,
    kernel: BitcrusherKernel,
    rate: f32,
    depth: f32,
    dry_wet: f32,
    in_audio: MonoInput,
    in_rate_cv: MonoInput,
    in_depth_cv: MonoInput,
    out_audio: MonoOutput,
}

impl Module for Bitcrusher {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("Bitcrusher", shape.clone())
            .mono_in("in")
            .mono_in("rate_cv")
            .mono_in("depth_cv")
            .mono_out("out")
            .float_param("rate", 0.0, 1.0, 1.0)
            .float_param("depth", 1.0, 32.0, 32.0)
            .float_param("dry_wet", 0.0, 1.0, 1.0)
    }

    fn prepare(env: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        let mut kernel = BitcrusherKernel::new();
        kernel.set_rate(1.0, env.sample_rate);
        kernel.set_depth(32.0);
        Self {
            instance_id,
            descriptor,
            sample_rate: env.sample_rate,
            kernel,
            rate: 1.0,
            depth: 32.0,
            dry_wet: 1.0,
            in_audio: MonoInput::default(),
            in_rate_cv: MonoInput::default(),
            in_depth_cv: MonoInput::default(),
            out_audio: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, params: &mut ParameterMap) {
        if let Some(ParameterValue::Float(v)) = params.get_scalar("rate") {
            self.rate = *v;
            self.kernel.set_rate(self.rate, self.sample_rate);
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("depth") {
            self.depth = *v;
            self.kernel.set_depth(self.depth);
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("dry_wet") {
            self.dry_wet = v.clamp(0.0, 1.0);
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_audio = MonoInput::from_ports(inputs, 0);
        self.in_rate_cv = MonoInput::from_ports(inputs, 1);
        self.in_depth_cv = MonoInput::from_ports(inputs, 2);
        self.out_audio = MonoOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let dry = pool.read_mono(&self.in_audio);
        let wet = self.kernel.tick(dry);
        let out = self.dry_wet * wet + (1.0 - self.dry_wet) * dry;
        pool.write_mono(&self.out_audio, out);
    }

    fn as_any(&self) -> &dyn std::any::Any { self }

    fn as_periodic(&mut self) -> Option<&mut dyn PeriodicUpdate> {
        Some(self)
    }
}

impl PeriodicUpdate for Bitcrusher {
    fn periodic_update(&mut self, pool: &CablePool<'_>) {
        let rate_cv = if self.in_rate_cv.is_connected() {
            pool.read_mono(&self.in_rate_cv)
        } else {
            0.0
        };
        let depth_cv = if self.in_depth_cv.is_connected() {
            pool.read_mono(&self.in_depth_cv)
        } else {
            0.0
        };

        let effective_rate = (self.rate + rate_cv).clamp(0.0, 1.0);
        self.kernel.set_rate(effective_rate, self.sample_rate);
        let effective_depth = (self.depth + depth_cv * 31.0).clamp(1.0, 32.0);
        self.kernel.set_depth(effective_depth);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::test_support::{assert_nearly, ModuleHarness, params};

    #[test]
    fn descriptor_shape() {
        let h = ModuleHarness::build::<Bitcrusher>(&[]);
        let desc = h.descriptor();
        assert_eq!(desc.inputs.len(), 3);
        assert_eq!(desc.outputs.len(), 1);
        assert_eq!(desc.inputs[0].name, "in");
        assert_eq!(desc.inputs[1].name, "rate_cv");
        assert_eq!(desc.inputs[2].name, "depth_cv");
        assert_eq!(desc.outputs[0].name, "out");
    }

    #[test]
    fn full_rate_full_depth_passes_through() {
        let mut h = ModuleHarness::build::<Bitcrusher>(
            params!["rate" => 1.0_f32, "depth" => 32.0_f32, "dry_wet" => 1.0_f32],
        );
        h.set_mono("in", 0.5);
        h.tick();
        assert_nearly!(0.5, h.read_mono("out"));
    }

    #[test]
    fn dry_wet_zero_passes_dry() {
        let mut h = ModuleHarness::build::<Bitcrusher>(
            params!["rate" => 0.5_f32, "depth" => 4.0_f32, "dry_wet" => 0.0_f32],
        );
        h.set_mono("in", 0.42);
        h.tick();
        assert_nearly!(0.42, h.read_mono("out"));
    }

    #[test]
    fn cv_reverts_to_base_on_zero() {
        let mut h = ModuleHarness::build::<Bitcrusher>(
            params!["rate" => 1.0_f32, "depth" => 32.0_f32, "dry_wet" => 1.0_f32],
        );
        // Apply rate CV to reduce rate significantly
        for _ in 0..32 {
            h.set_mono("in", 0.5);
            h.set_mono("rate_cv", -0.5);
            h.tick();
        }
        // Remove CV — should revert to base rate (1.0 = full rate, pass-through)
        for _ in 0..32 {
            h.set_mono("in", 0.5);
            h.set_mono("rate_cv", 0.0);
            h.tick();
        }
        // At full rate + full depth, output should equal input
        h.set_mono("in", 0.5);
        h.set_mono("rate_cv", 0.0);
        h.tick();
        assert_nearly!(0.5, h.read_mono("out"));
    }
}
