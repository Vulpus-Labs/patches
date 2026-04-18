use std::collections::HashMap;
use std::marker::PhantomData;
use patches_core::{
    AudioEnvironment, BuildError, InstanceId, Module, ModuleDescriptor, ModuleShape, ParameterMap,
};
use crate::file_processor::FileProcessor;
use crate::module_builder::{Builder, ModuleBuilder};

/// Type-erased file processor function pointer.
type FileProcessorFn = Box<
    dyn Fn(&AudioEnvironment, &ModuleShape, &str, &str) -> Result<Vec<f32>, String>
        + Send
        + Sync,
>;

pub struct Registry {
    builders: HashMap<String, Box<dyn ModuleBuilder>>,
    file_processors: HashMap<String, FileProcessorFn>,
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    pub fn new() -> Self {
        Self {
            builders: HashMap::new(),
            file_processors: HashMap::new(),
        }
    }

    pub fn register<T>(&mut self)
    where
        T: Module + 'static,
    {
        let name = T::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() }).module_name;
        self.builders
            .insert(name.to_string(), Box::new(Builder::<T>(PhantomData)));
    }

    /// Register a [`FileProcessor`] implementation for a module type.
    ///
    /// The module must already be registered via [`register`](Self::register).
    /// At plan-build time, the planner calls [`process_file`](Self::process_file)
    /// for any `ParameterValue::File` parameters on modules with a registered
    /// file processor.
    pub fn register_file_processor<T>(&mut self)
    where
        T: Module + FileProcessor + 'static,
    {
        let name = T::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() }).module_name;
        self.file_processors.insert(
            name.to_string(),
            Box::new(|env, shape, param_name, path| T::process_file(env, shape, param_name, path)),
        );
    }

    /// Register a pre-built `ModuleBuilder` under the given name.
    ///
    /// This is the entry point for dynamically-loaded plugins: the FFI loader
    /// constructs a `DylibModuleBuilder` and registers it here without going
    /// through the generic `register::<T>()` path.
    pub fn register_builder(&mut self, name: String, builder: Box<dyn ModuleBuilder>) {
        self.builders.insert(name, builder);
    }

    /// Returns an iterator over all registered module type names.
    pub fn module_names(&self) -> impl Iterator<Item = &str> {
        self.builders.keys().map(|s| s.as_str())
    }

    pub fn describe(&self, name: &str, shape: &ModuleShape) -> Result<ModuleDescriptor, BuildError> {
        self.builders
            .get(name)
            .map(|builder| builder.describe(shape))
            .ok_or_else(|| BuildError::UnknownModule { name: name.to_string(), origin: None })
    }

    /// Call the registered [`FileProcessor`] for the given module type.
    ///
    /// Returns `None` if no file processor is registered for `module_name`.
    /// Returns `Some(Err(...))` if the processor fails.
    pub fn process_file(
        &self,
        module_name: &str,
        env: &AudioEnvironment,
        shape: &ModuleShape,
        param_name: &str,
        path: &str,
    ) -> Option<Result<Vec<f32>, String>> {
        self.file_processors
            .get(module_name)
            .map(|f| f(env, shape, param_name, path))
    }

    /// Returns `true` if a [`FileProcessor`] is registered for the given module.
    pub fn has_file_processor(&self, module_name: &str) -> bool {
        self.file_processors.contains_key(module_name)
    }

    pub fn create(
        &self,
        name: &str,
        audio_environment: &AudioEnvironment,
        shape: &ModuleShape,
        params: &ParameterMap,
        instance_id: InstanceId,
    ) -> Result<Box<dyn Module>, BuildError> {
        let builder = self
            .builders
            .get(name)
            .ok_or_else(|| BuildError::UnknownModule { name: name.to_string(), origin: None })?;

        builder.build(audio_environment, shape, params, instance_id)
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
        fn describe(shape: &ModuleShape) -> ModuleDescriptor {
            ModuleDescriptor {
                module_name: "TestModule",
                shape: shape.clone(),
                inputs: vec![],
                outputs: vec![],
                parameters: vec![],
            }
        }

        fn prepare(
            _audio_environment: &AudioEnvironment,
            descriptor: ModuleDescriptor,
            instance_id: InstanceId,
        ) -> Self {
            Self {
                instance_id,
                descriptor,
            }
        }

        fn update_validated_parameters(&mut self, _params: &mut ParameterMap) {
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
        let mut registry = Registry::new();
        registry.register::<TestModule>();

        let shape = ModuleShape { channels: 2, length: 0, ..Default::default() };
        let params = ParameterMap::new();
        let audio_environment = AudioEnvironment { sample_rate: 44100.0, poly_voices: 16, periodic_update_interval: 32, hosted: false };
        let module = registry.create("TestModule", &audio_environment, &shape, &params, InstanceId::next()).unwrap();

        assert_eq!(module.descriptor().module_name, "TestModule");
        assert_eq!(module.descriptor().shape.channels, 2);
    }
}
