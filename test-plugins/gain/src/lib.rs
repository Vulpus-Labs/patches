use patches_core::cable_pool::CablePool;
use patches_core::cables::{InputPort, MonoInput, MonoOutput, OutputPort};
use patches_core::module_params;
use patches_core::modules::{InstanceId, ModuleDescriptor, ModuleShape};
use patches_core::param_frame::ParamView;
use patches_core::{AudioEnvironment, Module};

module_params! {
    Gain {
        gain: Float,
    }
}

pub struct Gain {
    descriptor: ModuleDescriptor,
    instance_id: InstanceId,
    gain: f32,
    input: MonoInput,
    output: MonoOutput,
}

fn describe(shape: &ModuleShape) -> ModuleDescriptor {
    ModuleDescriptor::new("Gain", shape.clone())
        .mono_in("in")
        .mono_out("out")
        .float_param(params::gain, 0.0, 2.0, 1.0)
}

impl Module for Gain {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        describe(shape)
    }

    fn prepare(
        _audio_environment: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
    ) -> Self {
        Self {
            descriptor,
            instance_id,
            gain: 1.0,
            input: MonoInput::default(),
            output: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        self.gain = p.get(params::gain);
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let input_val = pool.read_mono(&self.input);
        pool.write_mono(&self.output, input_val * self.gain);
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.input = MonoInput::from_ports(inputs, 0);
        self.output = MonoOutput::from_ports(outputs, 0);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

patches_ffi_common::export_plugin!(Gain, describe, "Gain");
