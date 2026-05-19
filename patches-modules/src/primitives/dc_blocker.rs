//! DC-blocker primitive (ADR 0076).
//!
//! Thin module wrapper over [`patches_dsp::DcBlocker`] — a one-pole
//! highpass at ~5 Hz. No parameters; the kernel's fixed cutoff stays.
//! Useful after waveshapers, bias injection, or any process that may
//! introduce DC offset.
//!
//! # Inputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `in` | mono | Audio input |
//!
//! # Outputs
//!
//! | Port  | Kind | Description       |
//! |-------|------|-------------------|
//! | `out` | mono | DC-blocked output |

use patches_core::modules::{CountAxis, ModuleDescriptorTemplate, PortTemplate};
use patches_core::param_frame::ParamView;
use patches_core::{
    AudioEnvironment, BuildError, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoInput, MonoOutput, OutputPort, StructuralParams,
};

use patches_dsp::DcBlocker as DcBlockerKernel;

pub struct DcBlocker {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    kernel: DcBlockerKernel,
    in_audio: MonoInput,
    out_audio: MonoOutput,
}

impl Module for DcBlocker {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "DcBlocker",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[PortTemplate::mono("in")],
            per_axis_inputs: &[],
            global_outputs: &[PortTemplate::mono("out")],
            per_axis_outputs: &[],
            realtime_params: &[],
            structural_params: &[],
            per_axis_realtime_params: &[],
            per_axis_structural_params: &[],
        };
        T
    }

    fn prepare(
        env: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
        _structural: &StructuralParams,
    ) -> Result<Self, BuildError> {
        Ok(Self {
            instance_id,
            descriptor,
            kernel: DcBlockerKernel::new(env.sample_rate),
            in_audio: MonoInput::default(),
            out_audio: MonoOutput::default(),
        })
    }

    fn update_validated_parameters(&mut self, _p: &ParamView<'_>) {}

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }
    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_audio = MonoInput::from_ports(inputs, 0);
        self.out_audio = MonoOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let x = pool.read_mono(&self.in_audio);
        pool.write_mono(&self.out_audio, self.kernel.process(x));
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::test_support::ModuleHarness;

    const SR: f32 = 44_100.0;
    const ENV: AudioEnvironment = AudioEnvironment {
        sample_rate: SR,
        poly_voices: 16,
        periodic_update_interval: 32,
        hosted: false,
    };

    fn build() -> ModuleHarness {
        ModuleHarness::build_full::<DcBlocker>(
            &[],
            ENV,
            patches_core::ModuleShape { channels: 0 },
        )
    }

    #[test]
    fn descriptor_shape() {
        let h = build();
        let desc = h.descriptor();
        assert_eq!(desc.inputs.len(), 1);
        assert_eq!(desc.outputs.len(), 1);
        assert_eq!(desc.inputs[0].name, "in");
        assert_eq!(desc.outputs[0].name, "out");
        assert!(desc.realtime_params.is_empty());
        assert!(desc.structural_params.is_empty());
    }

    #[test]
    fn removes_steady_dc() {
        // After ~1 s of constant input the one-pole highpass should
        // settle the output close to zero.
        let mut h = build();
        for _ in 0..(SR as usize) {
            h.set_mono("in", 1.0);
            h.tick();
        }
        h.set_mono("in", 1.0);
        h.tick();
        let out = h.read_mono("out");
        assert!(out.abs() < 0.01, "expected near-zero after settling, got {out}");
    }
}
