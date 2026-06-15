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
use std::ptr::NonNull;
use std::sync::{Arc, OnceLock};

use patches_core::build_error::BuildError;
use patches_core::cable_pool::CablePool;
use patches_core::cables::{InputPort, OutputPort, BACKPLANE_SIZE, SCRATCH_CAPACITY};
use patches_core::modules::{
    InstanceId, ModuleDescriptor, ModuleDescriptorTemplate, ModuleShape, ParameterMap,
    StructuralParams,
};
use patches_core::param_frame::ParamView;
use patches_core::param_layout::{compute_layout, ParamLayout};
use patches_core::{AudioEnvironment, ExpectInvariant, Module, ValidatedParamFrame};
use patches_ffi_common::abi::{Handle, HostEnv};
use patches_ffi_common::port_frame::{pack_ports_into, PortFrame, PortLayout};
use patches_core::registry::ModuleBuilder;

use crate::json;
use crate::types::{
    ABI_VERSION, FfiAudioEnvironment, FfiBytes, FfiPluginManifest,
    FfiPluginVTable, PREPARE_OK,
};
use patches_ffi_common::types::FFI_ENTRY_PANIC;
use patches_ffi_common::structural_frame::pack_structural;

// ── Host environment ─────────────────────────────────────────────────────────

extern "C" fn host_song_data_release(_id: u64) {
    // Song-data buffers do not yet cross the FFI boundary.
}

fn host_env() -> &'static HostEnv {
    static HOST_ENV: OnceLock<HostEnv> = OnceLock::new();
    HOST_ENV.get_or_init(|| HostEnv {
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
    handle: NonNull<c_void>,
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
        unsafe { (self.vtable.drop)(self.handle.as_ptr()) };
    }
}

impl DylibModule {
    /// Re-raise a panic the plugin-side fence (ADR 0051) caught and signalled
    /// via [`FFI_ENTRY_PANIC`]. The plugin's own `extern "C"` frame cannot
    /// unwind into the host (Rust 1.81 aborts), so the fence returns a
    /// sentinel and we re-panic *here*, on the host side of the boundary,
    /// where the enclosing tick / audio-callback fence turns it into a clean
    /// sticky halt (ticket 0995, E163).
    #[cold]
    #[inline(never)]
    fn ffi_panic(&self, entry: &str) -> ! {
        panic!(
            "plugin module '{}' panicked in {entry} (caught by plugin-side fence)",
            self.descriptor.module_name
        );
    }
}

impl Module for DylibModule {
    fn prepare(
        _audio_environment: &AudioEnvironment,
        _descriptor: ModuleDescriptor,
        _instance_id: InstanceId,
        _structural: &StructuralParams,
    ) -> Result<Self, BuildError>
    where
        Self: Sized,
    {
        unreachable!("DylibModule::prepare is not callable directly; use DylibModuleBuilder")
    }

    fn update_validated_parameters(&mut self, params: &ParamView<'_>) {
        let bytes = params.wire_bytes();
        let status = unsafe {
            (self.vtable.update_validated_parameters)(
                self.handle.as_ptr() as Handle,
                bytes.as_ptr(),
                bytes.len(),
                host_env() as *const HostEnv,
            )
        };
        if status == FFI_ENTRY_PANIC {
            self.ffi_panic("update_validated_parameters");
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let parts = pool.as_raw_parts_mut();
        // Plugin scratch view starts past the backplane (ticket 0870).
        // SAFETY: the planner refuses to size the scratch pool below
        // RESERVED_SLOTS (kernel.rs::init_scratch_pool clamps at
        // RESERVED_SLOTS), so parts.scratch_len >= RESERVED_SLOTS >
        // BACKPLANE_SIZE.
        debug_assert!(parts.scratch_len >= BACKPLANE_SIZE);
        let (scratch_ptr, scratch_len) = unsafe {
            (parts.scratch_ptr.add(BACKPLANE_SIZE), parts.scratch_len - BACKPLANE_SIZE)
        };
        let status = unsafe {
            (self.vtable.process)(
                self.handle.as_ptr(),
                scratch_ptr, scratch_len,
                parts.cycle_ptr, parts.cycle_len,
                parts.wi,
            )
        };
        // ADR 0051 / ticket 0995: the plugin-side fence caught a panic and
        // returned a sentinel rather than aborting across `extern "C"`.
        // Re-raise it so the host tick fence (PatchProcessor::tick) records a
        // clean halt against this module's breadcrumb slot.
        if status == FFI_ENTRY_PANIC {
            self.ffi_panic("process");
        }
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        // Pack into the preallocated per-instance `PortFrame` — no audio-
        // thread allocation (E104 ticket 0613). pack translates each
        // scratch-region cable_idx by BACKPLANE_SIZE into plugin-relative
        // space; cycle indices pass through (ticket 0870).
        pack_ports_into(0, inputs, outputs, BACKPLANE_SIZE, &mut self.port_frame)
            .expect_invariant("DylibModule::set_ports: shape matches the prepared PortLayout");
        let bytes = self.port_frame.bytes();
        let status = unsafe {
            (self.vtable.set_ports)(
                self.handle.as_ptr() as Handle,
                bytes.as_ptr(),
                bytes.len(),
                host_env() as *const HostEnv,
            )
        };
        if status == FFI_ENTRY_PANIC {
            self.ffi_panic("set_ports");
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn wants_periodic(&self) -> bool {
        self.vtable.supports_periodic != 0
    }

    fn scratch_base_offset(&self) -> usize {
        BACKPLANE_SIZE
    }

    fn periodic_update(&mut self, pool: &CablePool<'_>) {
        let parts = pool.as_raw_parts();
        debug_assert!(parts.scratch_len >= BACKPLANE_SIZE);
        let (scratch_ptr, scratch_len) = unsafe {
            (parts.scratch_ptr.add(BACKPLANE_SIZE), parts.scratch_len - BACKPLANE_SIZE)
        };
        let status = unsafe {
            (self.vtable.periodic_update)(
                self.handle.as_ptr(),
                scratch_ptr, scratch_len,
                parts.cycle_ptr, parts.cycle_len,
                parts.wi,
            )
        };
        if status == FFI_ENTRY_PANIC {
            self.ffi_panic("periodic_update");
        }
    }
}

// Compile-time guard against degenerate constants — subtraction in `process`
// and `periodic_update` must never underflow.
const _: () = assert!(BACKPLANE_SIZE < SCRATCH_CAPACITY);

// ── PrepareResult shim ───────────────────────────────────────────────────────

/// Decoded outcome of a plugin's `prepare` vtable call. Collapses the three
/// out-channels (status code, handle, error message) into a single Rust
/// `Result`, so callers don't have to re-derive the "got handle ⇒ status was
/// `PREPARE_OK`" invariant from raw out-params. The error string is already
/// UTF-8-decoded (lossy) and the plugin-allocated error buffer has been
/// freed via `free_bytes` before this returns.
enum PrepareResult {
    Ok(NonNull<c_void>),
    Err(String),
}

/// Invoke `vtable.prepare` and decode the (status, handle, error_bytes)
/// triple into a `PrepareResult`.
///
/// # Safety
///
/// `vtable` must conform to the FFI plugin ABI (the `vtable.prepare` /
/// `vtable.free_bytes` function pointers must be the genuine plugin entry
/// points; `desc_json_ptr` and `structural_ptr` must be valid for the
/// indicated lengths). The plugin promises that on `status != PREPARE_OK`
/// any returned `error_bytes` was allocated by the plugin and must be freed
/// via `vtable.free_bytes`; this function takes responsibility for that
/// free, so the caller never sees a dangling buffer.
unsafe fn call_prepare(
    vtable: &FfiPluginVTable,
    desc_json_ptr: *const u8,
    desc_json_len: usize,
    ffi_env: FfiAudioEnvironment,
    instance_id: InstanceId,
    structural_ptr: *const u8,
    structural_len: usize,
) -> PrepareResult {
    let mut handle: *mut c_void = std::ptr::null_mut();
    let mut error_bytes = FfiBytes::empty();
    let status = unsafe {
        (vtable.prepare)(
            desc_json_ptr,
            desc_json_len,
            ffi_env,
            instance_id.as_u64(),
            structural_ptr,
            structural_len,
            &mut handle as *mut *mut c_void,
            &mut error_bytes as *mut FfiBytes,
        )
    };

    if status == PREPARE_OK {
        if let Some(nn) = NonNull::new(handle) {
            return PrepareResult::Ok(nn);
        }
        // Plugin signalled OK but returned a null handle — protocol violation.
    }

    let message = if !error_bytes.ptr.is_null() && error_bytes.len > 0 {
        let bytes = unsafe { error_bytes.as_slice() };
        let msg = String::from_utf8_lossy(bytes).into_owned();
        unsafe { (vtable.free_bytes)(error_bytes) };
        msg
    } else {
        format!("plugin prepare failed (status {status})")
    };
    PrepareResult::Err(message)
}

// ── DylibModuleBuilder ───────────────────────────────────────────────────────

/// A `ModuleBuilder` backed by a loaded plugin library.
pub struct DylibModuleBuilder {
    vtable: FfiPluginVTable,
    /// Cached template fetched from the plugin's `module_template` vtable
    /// entry once at load time (ADR 0066, ticket 0795). Per-instance
    /// descriptors are built locally via [`ModuleDescriptorTemplate::build_channels`]
    /// without re-entering the plugin.
    template: ModuleDescriptorTemplate,
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
    fn template(&self) -> ModuleDescriptorTemplate {
        self.template
    }

    fn build(
        &self,
        audio_environment: &AudioEnvironment,
        shape: &ModuleShape,
        params: &ParameterMap,
        structural: &StructuralParams,
        instance_id: InstanceId,
    ) -> Result<Box<dyn Module>, BuildError> {
        let descriptor = self.template.build_channels(shape.channels as u32);

        let desc_json = json::serialize_module_descriptor(&descriptor);
        let ffi_env = FfiAudioEnvironment::from(audio_environment);

        let structural_blob = pack_structural(&descriptor, structural);

        // SAFETY: vtable was validated against ABI_VERSION at load time;
        // descriptor JSON and structural blob outlive the call.
        let handle = match unsafe {
            call_prepare(
                &self.vtable,
                desc_json.as_ptr(),
                desc_json.len(),
                ffi_env,
                instance_id,
                structural_blob.as_ptr(),
                structural_blob.len(),
            )
        } {
            PrepareResult::Ok(h) => h,
            PrepareResult::Err(message) => {
                return Err(BuildError::Custom {
                    module: descriptor.module_name,
                    message,
                    origin: None,
                });
            }
        };

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

        let filled = patches_core::parameter_map::ParameterMap::with_overrides(
            &patches_core::parameter_map::ParameterMap::declared_defaults(&module.descriptor),
            params.iter().map(|(n, i, v)| (n.to_string(), i, v.clone())),
        );
        let validated = ValidatedParamFrame::new(&module.descriptor, &filled)?;
        module.update_validated_parameters(&validated.view());

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
        // Pull the static template across the FFI exactly once.
        let bytes = unsafe { (vt.module_template)() };
        let slice = unsafe { bytes.as_slice() };
        let template = patches_ffi_common::json::deserialize_module_descriptor_template(slice)
            .map_err(|e| {
                format!(
                    "module_template JSON in {}: {e}",
                    path.display(),
                )
            })?;
        unsafe { (vt.free_bytes)(bytes) };

        let builder = DylibModuleBuilder {
            vtable: *vt,
            template,
            lib: Arc::clone(&lib_arc),
        };
        let descriptor = builder.template().build_channels(default_shape.channels as u32);
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
