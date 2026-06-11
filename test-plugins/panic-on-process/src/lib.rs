//! Fixture plugin whose `process` panics, to exercise the plugin-side
//! ADR 0051 panic fence in `export_plugin!` (E163 ticket 0995). The fence
//! must catch the unwind inside the `extern "C"` frame and signal the host,
//! which re-raises into its tick fence for a clean halt — never a process
//! abort.

use patches_sdk::cable_pool::CablePool;
use patches_sdk::modules::descriptor_template::{
    CountAxis, ModuleDescriptorTemplate,
};
use patches_sdk::modules::{InstanceId, ModuleDescriptor};
use patches_sdk::param_frame::ParamView;
use patches_sdk::{AudioEnvironment, Module};
use patches_sdk::{BuildError, StructuralParams};

pub struct PanicOnProcess {
    descriptor: ModuleDescriptor,
    instance_id: InstanceId,
}

impl Module for PanicOnProcess {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "PanicOnProcess",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[],
            per_axis_inputs: &[],
            global_outputs: &[],
            per_axis_outputs: &[],
            realtime_params: &[],
            structural_params: &[],
            per_axis_realtime_params: &[],
            per_axis_structural_params: &[],
        };
        T
    }

    fn prepare(
        _audio_environment: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
        _structural: &StructuralParams,
    ) -> Result<Self, BuildError> {
        Ok(Self { descriptor, instance_id })
    }

    fn update_validated_parameters(&mut self, _p: &ParamView<'_>) {}

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn process(&mut self, _pool: &mut CablePool<'_>) {
        panic!("boom in plugin process");
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

patches_sdk::export_plugin!(PanicOnProcess, "PanicOnProcess");
