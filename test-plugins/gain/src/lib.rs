use patches_sdk::cable_pool::CablePool;
use patches_sdk::cables::{InputPort, MonoInput, MonoOutput, OutputPort};
use patches_sdk::module_params;
use patches_sdk::modules::descriptor_template::{
    CountAxis, ModuleDescriptorTemplate, ParameterTemplate, PortTemplate,
};
use patches_sdk::modules::{InstanceId, ModuleDescriptor};
use patches_sdk::param_frame::ParamView;
use patches_sdk::ParameterKind;
use patches_sdk::{AudioEnvironment, Module};
use patches_sdk::{StructuralParams, BuildError};

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

impl Module for Gain {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "Gain",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[PortTemplate::mono("in")],
            per_axis_inputs: &[],
            global_outputs: &[PortTemplate::mono("out")],
            per_axis_outputs: &[],
            realtime_params: &[ParameterTemplate {
                name: params::gain.as_str(),
                kind: ParameterKind::Float { min: 0.0, max: 2.0, default: 1.0 },
            }],
            structural_params: &[],
            per_axis_realtime_params: &[],
            per_axis_structural_params: &[],
        };
        T
    }

    fn prepare(
        _audio_environment: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId, _structural: &StructuralParams,
    ) -> Result<Self, BuildError> { Ok({
        Self {
            descriptor,
            instance_id,
            gain: 1.0,
            input: MonoInput::default(),
            output: MonoOutput::default(),
        }
    })}

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

patches_sdk::export_plugin!(Gain, "Gain");
