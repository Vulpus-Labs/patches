//! Host-side FFI plugin loader.
//!
//! `DylibModule` wraps a plugin instance loaded from a shared library and
//! implements the `Module` trait by delegating to the plugin's vtable.
//! `DylibModuleBuilder` implements `ModuleBuilder` for registry integration.
//!
//! ADR 0045 Spike 7 Phase B (E104): audio-thread entry points take packed
//! `ParamFrame` / `PortFrame` wire bytes plus a shared `HostEnv`. JSON is
//! gone from the hot path; descriptor-hash is checked at load.

use std::ffi::c_void;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use patches_core::build_error::BuildError;
use patches_core::cable_pool::CablePool;
use patches_core::cables::{InputPort, OutputPort};
use patches_core::modules::{InstanceId, ModuleDescriptor, ModuleShape, ParameterMap};
use patches_core::param_frame::{pack_into, ParamFrame, ParamView, ParamViewIndex};
use patches_core::param_layout::{compute_layout, defaults_from_descriptor, ParamLayout};
use patches_core::{AudioEnvironment, Module};
use patches_ffi_common::abi::{Handle, HostEnv};
use patches_ffi_common::port_frame::{pack_ports_into, PortFrame, PortLayout};
use patches_registry::ModuleBuilder;

use crate::json;
use crate::types::{ABI_VERSION, FfiAudioEnvironment, FfiModuleShape, FfiPluginManifest, FfiPluginVTable};

// ── Host environment ─────────────────────────────────────────────────────────

extern "C" fn host_float_buffer_release(_id: u64) {
    // Placeholder for ArcTable wiring; release is a no-op until E107 lands
    // the allocator-trap audit path that exercises the real retain/release.
}

extern "C" fn host_song_data_release(_id: u64) {
    // See note above — song-data buffers do not yet cross the FFI boundary.
}

fn host_env() -> &'static HostEnv {
    static HOST_ENV: OnceLock<HostEnv> = OnceLock::new();
    HOST_ENV.get_or_init(|| HostEnv {
        float_buffer_release: host_float_buffer_release,
        song_data_release: host_song_data_release,
    })
}

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
    port_frame: PortFrame,
    _lib: Arc<libloading::Library>,
}

// Safety: the plugin's Module impl must be Send. Same contract as
// VST3/CLAP/AU/LV2 hosts — documented in ADR 0025.
unsafe impl Send for DylibModule {}

impl Drop for DylibModule {
    fn drop(&mut self) {
        unsafe { (self.vtable.drop)(self.handle) };
    }
}

impl Module for DylibModule {
    fn describe(_shape: &ModuleShape) -> ModuleDescriptor
    where
        Self: Sized,
    {
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

    fn update_validated_parameters(&mut self, params: &ParamView<'_>) {
        let bytes = params.wire_bytes();
        unsafe {
            (self.vtable.update_validated_parameters)(
                self.handle as Handle,
                bytes.as_ptr(),
                bytes.len(),
                host_env() as *const HostEnv,
            );
        }
    }

    fn update_parameters(&mut self, params: &ParameterMap) -> Result<(), BuildError> {
        // New ABI: control-thread validation happens host-side via the layout.
        // Pack defaults + overrides, then dispatch as a validated frame.
        let layout = compute_layout(&self.descriptor);
        let defaults = defaults_from_descriptor(&self.descriptor);
        let mut frame = ParamFrame::with_layout(&layout);
        pack_into(&layout, &defaults, params, &mut frame).map_err(|e| BuildError::Custom {
            module: self.descriptor.module_name,
            message: format!("{e:?}"),
            origin: None,
        })?;
        let index = ParamViewIndex::from_layout(&layout);
        let view = ParamView::new(&index, &frame);
        self.update_validated_parameters(&view);
        Ok(())
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
        // Pack into the preallocated per-instance `PortFrame` — no audio-
        // thread allocation (E104 ticket 0613).
        pack_ports_into(0, inputs, outputs, &mut self.port_frame)
            .expect("DylibModule::set_ports: shape mismatch vs. prepared PortLayout");
        let bytes = self.port_frame.bytes();
        unsafe {
            (self.vtable.set_ports)(
                self.handle as Handle,
                bytes.as_ptr(),
                bytes.len(),
                host_env() as *const HostEnv,
            );
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn wants_periodic(&self) -> bool {
        self.vtable.supports_periodic != 0
    }

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

// Safety: FfiPluginVTable contains only function pointers (Send+Sync) and
// Arc<Library> is Send+Sync.
unsafe impl Send for DylibModuleBuilder {}
unsafe impl Sync for DylibModuleBuilder {}

impl DylibModuleBuilder {
    pub fn vtable(&self) -> &FfiPluginVTable {
        &self.vtable
    }

    pub fn library_arc(&self) -> Arc<libloading::Library> {
        Arc::clone(&self.lib)
    }

    pub fn library_strong_count(&self) -> usize {
        Arc::strong_count(&self.lib)
    }

    pub fn module_version(&self) -> u32 {
        self.vtable.module_version
    }
}

impl ModuleBuilder for DylibModuleBuilder {
    fn describe(&self, shape: &ModuleShape) -> ModuleDescriptor {
        let ffi_shape = FfiModuleShape::from(shape);
        let bytes = unsafe { (self.vtable.describe)(ffi_shape) };
        let slice = unsafe { bytes.as_slice() };
        let desc = json::deserialize_module_descriptor(slice)
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
        let descriptor = self.describe(shape);

        let desc_json = json::serialize_module_descriptor(&descriptor);
        let ffi_env = FfiAudioEnvironment::from(audio_environment);

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
                message: "plugin prepare returned null".to_string(),
                origin: None,
            });
        }

        let port_layout = PortLayout::new(
            descriptor.inputs.len() as u32,
            descriptor.outputs.len() as u32,
        );
        let port_frame = PortFrame::with_layout(port_layout);

        let mut module = DylibModule {
            handle,
            vtable: self.vtable,
            descriptor,
            instance_id,
            port_frame,
            _lib: Arc::clone(&self.lib),
        };

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

/// Load a plugin bundle from a shared library at `path`.
///
/// Returns one `DylibModuleBuilder` per vtable in the plugin's manifest; all
/// builders share a single `Arc<libloading::Library>`. On ABI or descriptor-
/// hash mismatch the library is dropped before any plugin entry point is
/// called (E104 ticket 0614).
pub fn load_plugin(path: &Path) -> Result<Vec<DylibModuleBuilder>, String> {
    let lib = unsafe { libloading::Library::new(path) }
        .map_err(|e| format!("failed to load library {}: {e}", path.display()))?;

    let init_fn: libloading::Symbol<unsafe extern "C" fn() -> FfiPluginManifest> = unsafe {
        lib.get(b"patches_plugin_init")
    }.map_err(|e| format!("symbol 'patches_plugin_init' not found in {}: {e}", path.display()))?;

    let manifest = unsafe { init_fn() };

    if manifest.abi_version != ABI_VERSION {
        return Err(format!(
            "ABI version mismatch for {}: plugin has {}, host expects {}",
            path.display(),
            manifest.abi_version,
            ABI_VERSION,
        ));
    }

    if manifest.vtables.is_null() || manifest.count == 0 {
        return Err(format!(
            "plugin {} exposes an empty manifest",
            path.display(),
        ));
    }

    // Resolve the plugin's descriptor-hash symbol once; the exported fn is
    // stateless and returns the hash for the *first* vtable in the bundle.
    // Bundles with multiple modules each export `patches_plugin_descriptor_hash_<name>`;
    // we look those up per-entry below.
    let vtables: &[FfiPluginVTable] = unsafe {
        std::slice::from_raw_parts(manifest.vtables, manifest.count)
    };

    let lib_arc = Arc::new(lib);
    let mut builders: Vec<DylibModuleBuilder> = Vec::with_capacity(vtables.len());
    let mut names: Vec<String> = Vec::with_capacity(vtables.len());

    let default_shape = patches_core::ModuleShape::default();
    for vt in vtables.iter() {
        if vt.abi_version != ABI_VERSION {
            return Err(format!(
                "ABI version mismatch within bundle {}: vtable has {}, host expects {}",
                path.display(),
                vt.abi_version,
                ABI_VERSION,
            ));
        }
        let builder = DylibModuleBuilder {
            vtable: *vt,
            lib: Arc::clone(&lib_arc),
        };
        let descriptor = builder.describe(&default_shape);
        let name = descriptor.module_name.to_string();

        let host_hash = patches_ffi_common::descriptor_hash(&descriptor);
        let plugin_hash = read_plugin_descriptor_hash(&lib_arc, &name).map_err(|e| {
            format!(
                "module {name:?} in {}: descriptor_hash symbol not found: {e}",
                path.display(),
            )
        })?;
        if plugin_hash != host_hash {
            return Err(format!(
                "descriptor_hash mismatch for module {name:?} in {}: host {host_hash:#x}, plugin {plugin_hash:#x}",
                path.display(),
            ));
        }

        // Also sanity-check packed layout hash matches.
        let layout: ParamLayout = compute_layout(&descriptor);
        debug_assert_eq!(layout.descriptor_hash, host_hash);

        if names.iter().any(|n| n == &name) {
            return Err(format!(
                "duplicate module_name {name:?} within bundle {}",
                path.display(),
            ));
        }
        names.push(name);
        builders.push(builder);
    }

    Ok(builders)
}

/// Look up the plugin's `patches_plugin_descriptor_hash_<module>` symbol.
fn read_plugin_descriptor_hash(
    lib: &libloading::Library,
    module_name: &str,
) -> Result<u64, libloading::Error> {
    let sym = format!("patches_plugin_descriptor_hash_{module_name}");
    let f: libloading::Symbol<unsafe extern "C" fn() -> u64> =
        unsafe { lib.get(sym.as_bytes()) }?;
    Ok(unsafe { f() })
}
