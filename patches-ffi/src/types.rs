use std::ffi::c_void;

use patches_core::cables::CableValue;

// Re-export shared types from patches-ffi-common
pub use patches_ffi_common::{
    ABI_VERSION, FfiAudioEnvironment, FfiBytes, FfiInputPort, FfiModuleShape,
    FfiOutputPort, PORT_TAG_MONO, PORT_TAG_POLY,
};

// ── FfiPluginVTable ──────────────────────────────────────────────────────────

/// The C ABI contract between host and plugin.
///
/// A plugin exports `patches_plugin_init() -> FfiPluginVTable` which the host
/// calls once at load time. All subsequent interaction goes through these
/// function pointers.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FfiPluginVTable {
    pub abi_version: u32,
    pub supports_periodic: i32,

    /// Return the module descriptor as JSON bytes for the given shape.
    pub describe: unsafe extern "C" fn(shape: FfiModuleShape) -> FfiBytes,

    /// Create a new module instance. Takes JSON-serialized ModuleDescriptor,
    /// FfiAudioEnvironment, and instance_id. Returns an opaque handle.
    pub prepare: unsafe extern "C" fn(
        descriptor_json: *const u8,
        descriptor_json_len: usize,
        env: FfiAudioEnvironment,
        instance_id: u64,
    ) -> *mut c_void,

    /// Apply pre-validated parameters (JSON-serialized ParameterMap).
    pub update_validated_parameters: unsafe extern "C" fn(
        handle: *mut c_void,
        params_json: *const u8,
        params_json_len: usize,
    ),

    /// Validate and apply parameters. Returns 0 on success, 1 on error.
    /// On error, `error_out` is filled with the error message bytes.
    pub update_parameters: unsafe extern "C" fn(
        handle: *mut c_void,
        params_json: *const u8,
        params_json_len: usize,
        error_out: *mut FfiBytes,
    ) -> i32,

    /// Process one sample. Zero overhead: raw CablePool pointer pass-through.
    pub process: unsafe extern "C" fn(
        handle: *mut c_void,
        pool_ptr: *mut [CableValue; 2],
        pool_len: usize,
        write_index: usize,
    ),

    /// Deliver resolved port objects to the module.
    pub set_ports: unsafe extern "C" fn(
        handle: *mut c_void,
        inputs: *const FfiInputPort,
        inputs_len: usize,
        outputs: *const FfiOutputPort,
        outputs_len: usize,
    ),

    /// Periodic coefficient update. Returns 1 if the module supports it, 0 if not.
    pub periodic_update: unsafe extern "C" fn(
        handle: *mut c_void,
        pool_ptr: *const [CableValue; 2],
        pool_len: usize,
        write_index: usize,
    ) -> i32,

    /// Return the live module descriptor as JSON bytes.
    pub descriptor: unsafe extern "C" fn(handle: *mut c_void) -> FfiBytes,

    /// Return the instance ID as a raw u64.
    pub instance_id: unsafe extern "C" fn(handle: *mut c_void) -> u64,

    /// Drop the module instance (runs `Drop`, joins threads).
    pub drop: unsafe extern "C" fn(handle: *mut c_void),

    /// Free a plugin-allocated `FfiBytes` buffer.
    pub free_bytes: unsafe extern "C" fn(bytes: FfiBytes),
}
