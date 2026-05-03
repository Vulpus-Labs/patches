use std::collections::HashMap;
use std::marker::PhantomData;
use patches_core::modules::ModuleDescriptorTemplate;
use patches_core::{
    AudioEnvironment, BuildError, InstanceId, Module, ModuleDescriptor, ModuleShape, ParameterMap,
    StructuralParams,
};
use crate::module_builder::{Builder, ModuleBuilder};

pub struct Registry {
    builders: HashMap<String, Box<dyn ModuleBuilder>>,
    /// Cached static descriptor template per registered module name.
    /// Empty for legacy FFI builders that have not migrated to the
    /// template vtable; describe() falls back to the builder's
    /// per-module describe in that case (ADR 0066, E132).
    templates: HashMap<String, ModuleDescriptorTemplate>,
    /// Per-builder module version (semver-packed). Built-in (non-FFI)
    /// registrations use `0` so any FFI plugin v >= 1 can shadow them.
    versions: HashMap<String, u32>,
}

/// Outcome of a version-aware registry insert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterOutcome {
    /// New builder under a previously unused name.
    Inserted,
    /// Existing builder replaced by a strictly higher version.
    Replaced { from: u32, to: u32 },
    /// Candidate skipped: existing version is equal or higher.
    Skipped { existing: u32, candidate: u32 },
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
            templates: HashMap::new(),
            versions: HashMap::new(),
        }
    }

    pub fn register<T>(&mut self)
    where
        T: Module + 'static,
    {
        let template = T::template();
        assert!(
            !template.name.is_empty(),
            "Registry::register::<T>: T::template() must be non-empty (declare a const TEMPLATE)",
        );
        let name = template.name.to_string();
        self.versions.insert(name.clone(), 0);
        self.templates.insert(name.clone(), template);
        self.builders
            .insert(name, Box::new(Builder::<T>(PhantomData)));
    }

    /// Register a pre-built `ModuleBuilder` under the given name.
    ///
    /// This is the entry point for dynamically-loaded plugins: the FFI loader
    /// constructs a `DylibModuleBuilder` and registers it here without going
    /// through the generic `register::<T>()` path.
    pub fn register_builder(&mut self, name: String, builder: Box<dyn ModuleBuilder>) {
        self.versions.insert(name.clone(), 0);
        self.templates.insert(name.clone(), builder.template());
        self.builders.insert(name, builder);
    }

    /// Version-aware registration. Inserts the builder under `name` only if
    /// no prior version exists or the candidate's `version` is strictly
    /// greater than the existing one. Equal or lower versions are a no-op
    /// and are reported via the returned [`RegisterOutcome`].
    pub fn register_builder_versioned(
        &mut self,
        name: String,
        builder: Box<dyn ModuleBuilder>,
        version: u32,
    ) -> RegisterOutcome {
        match self.versions.get(&name).copied() {
            None => {
                self.versions.insert(name.clone(), version);
                self.templates.insert(name.clone(), builder.template());
                self.builders.insert(name, builder);
                RegisterOutcome::Inserted
            }
            Some(existing) if version > existing => {
                self.versions.insert(name.clone(), version);
                self.templates.insert(name.clone(), builder.template());
                self.builders.insert(name, builder);
                RegisterOutcome::Replaced { from: existing, to: version }
            }
            Some(existing) => RegisterOutcome::Skipped { existing, candidate: version },
        }
    }

    /// Current recorded version for a registered module name, if any.
    pub fn module_version(&self, name: &str) -> Option<u32> {
        self.versions.get(name).copied()
    }

    /// Returns an iterator over all registered module type names.
    pub fn module_names(&self) -> impl Iterator<Item = &str> {
        self.builders.keys().map(|s| s.as_str())
    }

    pub fn describe(&self, name: &str, shape: &ModuleShape) -> Result<ModuleDescriptor, BuildError> {
        validate_shape(name, shape)?;
        if !self.builders.contains_key(name) {
            return Err(BuildError::UnknownModule { name: name.to_string(), origin: None });
        }
        let template = self.templates.get(name).expect("template recorded at register time");
        Ok(template.build_channels(shape.channels as u32))
    }

    /// Static descriptor template for a registered module.
    pub fn template(&self, name: &str) -> Option<&ModuleDescriptorTemplate> {
        self.templates.get(name)
    }

    pub fn create(
        &self,
        name: &str,
        audio_environment: &AudioEnvironment,
        shape: &ModuleShape,
        params: &ParameterMap,
        structural: &StructuralParams,
        instance_id: InstanceId,
    ) -> Result<Box<dyn Module>, BuildError> {
        // No `validate_shape` here: shape-independent modules record
        // `ModuleShape { channels: 0 }` as a sentinel in their descriptor,
        // and the planner re-passes that descriptor shape into `create`
        // (state/mod.rs `NodeDecision::Install`). Validation belongs at
        // describe-time, where the shape originates from authored DSL.
        let builder = self
            .builders
            .get(name)
            .ok_or_else(|| BuildError::UnknownModule { name: name.to_string(), origin: None })?;

        builder.build(audio_environment, shape, params, structural, instance_id)
    }
}

/// Reject shapes that violate invariants every module relies on. `channels`
/// is the count of `*_multi` port families and parameter array entries —
/// 0 means the descriptor would carry no entries for those families and
/// the module would silently behave as if they didn't exist. Treat it as
/// a planner-stage error.
fn validate_shape(name: &str, shape: &ModuleShape) -> Result<(), BuildError> {
    if let Err(reason) = shape.validate() {
        return Err(BuildError::InvalidShape {
            module: name.to_string(),
            reason: reason.to_string(),
            origin: None,
        });
    }
    Ok(())
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

    struct AltModule { instance_id: InstanceId, descriptor: ModuleDescriptor }
    impl Module for AltModule {
        fn template() -> ModuleDescriptorTemplate {
            <TestModule as Module>::template()
        }
        fn prepare(_e: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId, _structural: &StructuralParams) -> Result<Self, BuildError> { Ok(Self { instance_id, descriptor })}
        fn update_validated_parameters(&mut self, _p: &patches_core::param_frame::ParamView<'_>) {}
        fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
        fn instance_id(&self) -> InstanceId { self.instance_id }
        fn process(&mut self, _p: &mut patches_core::CablePool<'_>) {}
        fn as_any(&self) -> &dyn std::any::Any { self }
    }

    fn make_builder() -> Box<dyn ModuleBuilder> {
        Box::new(Builder::<AltModule>(PhantomData))
    }

    #[test]
    fn register_versioned_insert_replace_skip() {
        let mut r = Registry::new();
        assert_eq!(
            r.register_builder_versioned("M".into(), make_builder(), 10),
            RegisterOutcome::Inserted,
        );
        assert_eq!(r.module_version("M"), Some(10));
        assert_eq!(
            r.register_builder_versioned("M".into(), make_builder(), 20),
            RegisterOutcome::Replaced { from: 10, to: 20 },
        );
        assert_eq!(r.module_version("M"), Some(20));
        assert_eq!(
            r.register_builder_versioned("M".into(), make_builder(), 5),
            RegisterOutcome::Skipped { existing: 20, candidate: 5 },
        );
        assert_eq!(
            r.register_builder_versioned("M".into(), make_builder(), 20),
            RegisterOutcome::Skipped { existing: 20, candidate: 20 },
        );
        assert_eq!(r.module_version("M"), Some(20));
    }

    #[test]
    fn build_a_module() {
        let mut registry = Registry::new();
        registry.register::<TestModule>();

        let shape = ModuleShape { channels: 2 };
        let params = ParameterMap::new();
        let audio_environment = AudioEnvironment { sample_rate: 44100.0, poly_voices: 16, periodic_update_interval: 32, hosted: false };
        let module = registry.create("TestModule", &audio_environment, &shape, &params, &StructuralParams::new(), InstanceId::next()).unwrap();

        assert_eq!(module.descriptor().module_name, "TestModule");
        assert_eq!(module.descriptor().shape.channels, 2);
    }
}
