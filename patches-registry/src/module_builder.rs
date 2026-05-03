use std::marker::PhantomData;
use patches_core::modules::ModuleDescriptorTemplate;
use patches_core::{
    AudioEnvironment, BuildError, InstanceId, Module, ModuleShape, ParameterMap,
    StructuralParams,
};

pub trait ModuleBuilder: Send + Sync {
    /// Static descriptor template for this module type (ADR 0066).
    /// Built-in builders return the module's `Module::template()`;
    /// FFI-loaded plugins return the deserialized template fetched
    /// from the plugin's `module_template` vtable entry at load time.
    fn template(&self) -> ModuleDescriptorTemplate;

    fn build(
        &self,
        audio_environment: &AudioEnvironment,
        shape: &ModuleShape,
        params: &ParameterMap,
        structural: &StructuralParams,
        instance_id: InstanceId,
    ) -> Result<Box<dyn Module>, BuildError>;
}

pub struct Builder<T>(pub PhantomData<fn() -> T>);

impl<T> ModuleBuilder for Builder<T>
where
    T: Module + 'static,
{
    fn template(&self) -> ModuleDescriptorTemplate {
        T::template()
    }

    fn build(
        &self,
        audio_environment: &AudioEnvironment,
        shape: &ModuleShape,
        params: &ParameterMap,
        structural: &StructuralParams,
        instance_id: InstanceId,
    ) -> Result<Box<dyn Module>, BuildError> {
        Ok(Box::new(T::build(audio_environment, shape, params, structural, instance_id)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::{InstanceId, ModuleDescriptor};

    struct TestModule {
        instance_id: InstanceId,
        descriptor: ModuleDescriptor,
    }

    impl Module for TestModule {
        fn template() -> ModuleDescriptorTemplate {
            use patches_core::modules::descriptor_template::{CountAxis, ModuleDescriptorTemplate};
            ModuleDescriptorTemplate {
                name: "TestModule",
                axes: &[CountAxis::CHANNELS],
                global_inputs: &[],
                per_axis_inputs: &[],
                global_outputs: &[],
                per_axis_outputs: &[],
                realtime_params: &[],
                structural_params: &[],
                per_axis_realtime_params: &[],
                per_axis_structural_params: &[],
            }
        }

        fn prepare(
            _audio_environment: &AudioEnvironment,
            descriptor: ModuleDescriptor,
            instance_id: InstanceId, _structural: &StructuralParams,
        ) -> Result<Self, BuildError> { Ok({
            Self {
                instance_id,
                descriptor,
            }
        })}

        fn update_validated_parameters(&mut self, _params: &patches_core::param_frame::ParamView<'_>) {
        }

        fn descriptor(&self) -> &ModuleDescriptor {
            &self.descriptor
        }

        fn instance_id(&self) -> InstanceId {
            self.instance_id
        }

        fn process(&mut self, _pool: &mut patches_core::CablePool<'_>) {}

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[test]
    fn build_a_module() {
        let audio_environment = AudioEnvironment { sample_rate: 44100.0, poly_voices: 16, periodic_update_interval: 32, hosted: false };
        let shape = ModuleShape { channels: 2 };
        let params = ParameterMap::new();
        let builder = Builder::<TestModule>(PhantomData);
        let module = builder.build(&audio_environment, &shape, &params, &StructuralParams::new(), InstanceId::next()).unwrap();

        assert_eq!(module.descriptor().module_name, "TestModule");
        assert_eq!(module.descriptor().shape.channels, 2);
    }
}