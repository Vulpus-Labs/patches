//! Host-side FFI plugin loader.
//!
//! `DylibModule` wraps a plugin instance loaded from a shared library and
//! implements the `Module` trait by delegating to the plugin's vtable.
//! `DylibModuleBuilder` implements `ModuleBuilder` for registry integration.

use std::ffi::c_void;
use std::path::Path;
use std::sync::Arc;

use patches_core::build_error::BuildError;
use patches_core::cable_pool::CablePool;
use patches_core::cables::{InputPort, OutputPort};
use patches_core::modules::module::PeriodicUpdate;
use patches_core::modules::{InstanceId, ModuleDescriptor, ModuleShape, ParameterMap};
use patches_core::{AudioEnvironment, Module};
use patches_core::registries::ModuleBuilder;

use crate::json;
use crate::types::{ABI_VERSION, FfiAudioEnvironment, FfiBytes, FfiInputPort, FfiModuleShape, FfiOutputPort, FfiPluginVTable};

// ── DylibModule ──────────────────────────────────────────────────────────────

/// A `Module` implementation backed by a dynamically-loaded plugin.
///
/// Drop ordering is critical: `handle` and `vtable` are declared before `_lib`
/// so that `vtable.drop(handle)` completes (joining any plugin threads) before
/// `Arc<Library>` decrements and potentially unloads the library.
pub struct DylibModule {
    handle: *mut c_void,
    vtable: FfiPluginVTable,
    descriptor: ModuleDescriptor,
    instance_id: InstanceId,
    _lib: Arc<libloading::Library>,
}

// Safety: the plugin's Module impl must be Send. This is the same contract
// as VST3/CLAP/AU/LV2 hosts — documented in ADR 0025.
unsafe impl Send for DylibModule {}

impl Drop for DylibModule {
    fn drop(&mut self) {
        // Calls the plugin's drop, which joins any spawned threads.
        // This must complete before Arc<Library> decrements (guaranteed by
        // Rust's field drop order: handle is declared before _lib).
        unsafe { (self.vtable.drop)(self.handle) };
    }
}

impl Module for DylibModule {
    fn describe(_shape: &ModuleShape) -> ModuleDescriptor
    where
        Self: Sized,
    {
        // Not callable on the type itself — use DylibModuleBuilder::describe.
        unreachable!("DylibModule::describe is not callable directly; use DylibModuleBuilder")
    }

    fn prepare(
        _audio_environment: &AudioEnvironment,
        _descriptor: ModuleDescriptor,
        _instance_id: InstanceId,
    ) -> Self
    where
        Self: Sized,
    {
        unreachable!("DylibModule::prepare is not callable directly; use DylibModuleBuilder")
    }

    fn update_validated_parameters(&mut self, params: &mut ParameterMap) {
        let json = json::serialize_parameter_map(params);
        unsafe {
            (self.vtable.update_validated_parameters)(
                self.handle,
                json.as_ptr(),
                json.len(),
            );
        }
    }

    fn update_parameters(&mut self, params: &ParameterMap) -> Result<(), BuildError> {
        let json = json::serialize_parameter_map(params);
        let mut error_out = FfiBytes::empty();
        let result = unsafe {
            (self.vtable.update_parameters)(
                self.handle,
                json.as_ptr(),
                json.len(),
                &mut error_out,
            )
        };
        if result != 0 {
            let msg = unsafe { json::deserialize_error(error_out.as_slice()) };
            unsafe { (self.vtable.free_bytes)(error_out) };
            Err(BuildError::Custom {
                module: self.descriptor.module_name,
                message: msg, origin: None,
            })
        } else {
            Ok(())
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let (ptr, len, wi) = pool.as_raw_parts_mut();
        unsafe { (self.vtable.process)(self.handle, ptr, len, wi) };
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        let ffi_inputs: Vec<FfiInputPort> = inputs.iter().map(FfiInputPort::from).collect();
        let ffi_outputs: Vec<FfiOutputPort> = outputs.iter().map(FfiOutputPort::from).collect();
        unsafe {
            (self.vtable.set_ports)(
                self.handle,
                ffi_inputs.as_ptr(),
                ffi_inputs.len(),
                ffi_outputs.as_ptr(),
                ffi_outputs.len(),
            );
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_periodic(&mut self) -> Option<&mut dyn PeriodicUpdate> {
        if self.vtable.supports_periodic != 0 {
            Some(self)
        } else {
            None
        }
    }
}

impl PeriodicUpdate for DylibModule {
    fn periodic_update(&mut self, pool: &CablePool<'_>) {
        let (ptr, len, wi) = pool.as_raw_parts();
        unsafe { (self.vtable.periodic_update)(self.handle, ptr, len, wi) };
    }
}

// ── DylibModuleBuilder ───────────────────────────────────────────────────────

/// A `ModuleBuilder` backed by a loaded plugin library.
pub struct DylibModuleBuilder {
    vtable: FfiPluginVTable,
    lib: Arc<libloading::Library>,
}

// Safety: FfiPluginVTable contains only function pointers (which are Send+Sync)
// and the Arc<Library> is Send+Sync.
unsafe impl Send for DylibModuleBuilder {}
unsafe impl Sync for DylibModuleBuilder {}

impl DylibModuleBuilder {
    /// Get the vtable for use in tests or advanced scenarios.
    pub fn vtable(&self) -> &FfiPluginVTable {
        &self.vtable
    }
}

impl ModuleBuilder for DylibModuleBuilder {
    fn describe(&self, shape: &ModuleShape) -> ModuleDescriptor {
        let ffi_shape = FfiModuleShape::from(shape);
        let bytes = unsafe { (self.vtable.describe)(ffi_shape) };
        let json = unsafe { bytes.as_slice() };
        let desc = json::deserialize_module_descriptor(json)
            .expect("plugin describe returned invalid JSON");
        unsafe { (self.vtable.free_bytes)(bytes) };
        desc
    }

    fn build(
        &self,
        audio_environment: &AudioEnvironment,
        shape: &ModuleShape,
        params: &ParameterMap,
        instance_id: InstanceId,
    ) -> Result<Box<dyn Module>, BuildError> {
        // 1. Describe
        let descriptor = self.describe(shape);

        // 2. Serialize descriptor for prepare
        let desc_json = json::serialize_module_descriptor(&descriptor);
        let ffi_env = FfiAudioEnvironment::from(audio_environment);

        // 3. Prepare (create instance)
        let handle = unsafe {
            (self.vtable.prepare)(
                desc_json.as_ptr(),
                desc_json.len(),
                ffi_env,
                instance_id.as_u64(),
            )
        };

        if handle.is_null() {
            return Err(BuildError::Custom {
                module: descriptor.module_name,
                message: "plugin prepare returned null".to_string(), origin: None,
            });
        }

        let mut module = DylibModule {
            handle,
            vtable: self.vtable,
            descriptor,
            instance_id,
            _lib: Arc::clone(&self.lib),
        };

        // 4. Fill defaults and validate+apply parameters
        let mut filled = params.clone();
        for param_desc in module.descriptor.parameters.iter() {
            filled.get_or_insert(param_desc.name, param_desc.index, || {
                param_desc.parameter_type.default_value()
            });
        }
        module.update_parameters(&filled)?;

        Ok(Box::new(module))
    }
}

// ── load_plugin ──────────────────────────────────────────────────────────────

/// Load a plugin from a shared library at `path`.
///
/// Returns a `DylibModuleBuilder` that can be registered in the `Registry`.
///
/// # Errors
/// Returns an error if the library cannot be opened, the init symbol is
/// missing, or the ABI version does not match.
pub fn load_plugin(path: &Path) -> Result<DylibModuleBuilder, String> {
    let lib = unsafe { libloading::Library::new(path) }
        .map_err(|e| format!("failed to load library {}: {e}", path.display()))?;

    let init_fn: libloading::Symbol<unsafe extern "C" fn() -> FfiPluginVTable> = unsafe {
        lib.get(b"patches_plugin_init")
    }.map_err(|e| format!("symbol 'patches_plugin_init' not found in {}: {e}", path.display()))?;

    let vtable = unsafe { init_fn() };

    if vtable.abi_version != ABI_VERSION {
        return Err(format!(
            "ABI version mismatch for {}: plugin has {}, host expects {}",
            path.display(),
            vtable.abi_version,
            ABI_VERSION,
        ));
    }

    Ok(DylibModuleBuilder {
        vtable,
        lib: Arc::new(lib),
    })
}
