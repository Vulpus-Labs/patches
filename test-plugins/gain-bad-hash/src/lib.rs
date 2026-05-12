//! Gain fixture that reports a deliberately wrong descriptor hash.
//! E107 ticket 0622.

use patches_sdk::cable_pool::CablePool;
use patches_sdk::modules::descriptor_template::{
    CountAxis, ModuleDescriptorTemplate, ParameterTemplate, PortTemplate,
};
use patches_sdk::modules::{InstanceId, ModuleDescriptor};
use patches_sdk::param_frame::ParamView;
use patches_sdk::ParameterKind;
use patches_sdk::{AudioEnvironment, BuildError, Module, StructuralParams};

pub struct Gain;

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
                name: "gain",
                kind: ParameterKind::Float { min: 0.0, max: 2.0, default: 1.0 },
            }],
            structural_params: &[],
            per_axis_realtime_params: &[],
            per_axis_structural_params: &[],
        };
        T
    }

    fn prepare(
        _env: &AudioEnvironment,
        _descriptor: ModuleDescriptor,
        _instance_id: InstanceId,
        _structural: &StructuralParams,
    ) -> Result<Self, BuildError> {
        Err(BuildError::Custom { module: "Gain", message: "stub".into(), origin: None })
    }
    fn update_validated_parameters(&mut self, _p: &ParamView<'_>) {}
    fn descriptor(&self) -> &ModuleDescriptor { unreachable!() }
    fn instance_id(&self) -> InstanceId { unreachable!() }
    fn process(&mut self, _pool: &mut CablePool<'_>) {}
    fn as_any(&self) -> &dyn std::any::Any { self }
}

patches_sdk::export_plugin_with_hash_override!(
    Gain,
    "Gain",
    0xDEAD_BEEF_DEAD_BEEFu64
);
