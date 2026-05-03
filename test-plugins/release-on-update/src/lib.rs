//! Fixture plugin retained as a hand-written ABI surface that exposes
//! `HostEnv` to `update_validated_parameters`. The stock `export_plugin!`
//! macro drops the env argument; this fixture stays in tree so future
//! ABI tests can reach the env without re-deriving the boilerplate.

use std::ffi::c_void;

use patches_core::cables::CableValue;
use patches_core::modules::descriptor_template::{
    CountAxis, ModuleDescriptorTemplate, ParameterTemplate,
};
use patches_core::modules::{ModuleDescriptor};
use patches_core::param_frame::ParamViewIndex;
use patches_core::param_layout::compute_layout;
use patches_core::ParameterKind;
use patches_ffi_common::abi::{Handle, HostEnv};
use patches_ffi_common::port_frame::PortLayout;
use patches_ffi_common::sdk::{decode_param_frame, PluginInstance};
use patches_ffi_common::types::{
    FfiAudioEnvironment, FfiBytes, FfiPluginManifest, FfiPluginVTable,
    ABI_VERSION,
};
use patches_core::{StructuralParams, BuildError};
use patches_ffi_common::{descriptor_hash, json};

pub struct Stub;

const TEMPLATE: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
    name: "ReleaseOnUpdate",
    axes: &[CountAxis::CHANNELS],
    global_inputs: &[],
    per_axis_inputs: &[],
    global_outputs: &[],
    per_axis_outputs: &[],
    realtime_params: &[],
    structural_params: &[ParameterTemplate {
        name: "s",
        kind: ParameterKind::File { extensions: &["wav"] },
    }],
    per_axis_realtime_params: &[],
    per_axis_structural_params: &[],
};

impl patches_core::Module for Stub {
    fn template() -> ModuleDescriptorTemplate { TEMPLATE }
    fn prepare(
        _env: &patches_core::AudioEnvironment,
        _d: ModuleDescriptor,
        _id: patches_core::modules::InstanceId, _structural: &StructuralParams,
    ) -> Result<Self, BuildError> { Ok({
        Stub
    })}
    fn update_validated_parameters(&mut self, _p: &patches_core::param_frame::ParamView<'_>) {}
    fn descriptor(&self) -> &ModuleDescriptor {
        unreachable!()
    }
    fn instance_id(&self) -> patches_core::modules::InstanceId {
        patches_core::modules::InstanceId::from_raw(0)
    }
    fn process(&mut self, _: &mut patches_core::cable_pool::CablePool<'_>) {}
    fn set_ports(&mut self, _: &[patches_core::InputPort], _: &[patches_core::OutputPort]) {}
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn __rop_module_template() -> FfiBytes {
    FfiBytes::from_vec(json::serialize_module_descriptor_template(&TEMPLATE))
}

/// # Safety
/// Hand-written ABI fixture; pointers must be valid for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __rop_prepare(
    descriptor_json: *const u8,
    descriptor_json_len: usize,
    _env: FfiAudioEnvironment,
    _instance_id: u64,
    _structural_blob: *const u8,
    _structural_blob_len: usize,
    out_handle: *mut *mut c_void,
    out_error: *mut FfiBytes,
) -> i32 {
    unsafe {
        if !out_handle.is_null() {
            *out_handle = std::ptr::null_mut();
        }
        if !out_error.is_null() {
            *out_error = FfiBytes::empty();
        }
    }
    let slice =
        unsafe { std::slice::from_raw_parts(descriptor_json, descriptor_json_len) };
    let descriptor = match json::deserialize_module_descriptor(slice) {
        Ok(d) => d,
        Err(_) => return patches_ffi_common::types::PREPARE_ERR_DESCRIPTOR_JSON,
    };
    let layout = compute_layout(&descriptor);
    let param_index = ParamViewIndex::from_layout(&layout);
    let port_layout = PortLayout::new(
        descriptor.inputs.len() as u32,
        descriptor.outputs.len() as u32,
    );
    let inst = Box::new(PluginInstance::<Stub> {
        module: Stub,
        param_index,
        port_layout,
        input_buf: Vec::new(),
        output_buf: Vec::new(),
    });
    unsafe {
        *out_handle = Box::into_raw(inst) as *mut c_void;
    }
    patches_ffi_common::types::PREPARE_OK
}

/// # Safety
/// `handle` must be a live instance; `bytes` must be valid for `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __rop_update(
    handle: Handle,
    bytes: *const u8,
    len: usize,
    env: *const HostEnv,
) {
    let inst = unsafe { &mut *(handle as *mut PluginInstance<Stub>) };
    let slice = unsafe { std::slice::from_raw_parts(bytes, len) };
    let _view = match decode_param_frame(slice, &inst.param_index) {
        Ok(v) => v,
        Err(_) => return,
    };
    let _ = env;
}

#[unsafe(no_mangle)]
pub extern "C" fn __rop_set_ports(
    _h: Handle,
    _b: *const u8,
    _l: usize,
    _e: *const HostEnv,
) {
}

#[unsafe(no_mangle)]
pub extern "C" fn __rop_process(
    _h: *mut c_void,
    _p: *mut [CableValue; 2],
    _l: usize,
    _w: usize,
) {
}

#[unsafe(no_mangle)]
pub extern "C" fn __rop_periodic(
    _h: *mut c_void,
    _p: *const [CableValue; 2],
    _l: usize,
    _w: usize,
) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn __rop_drop(h: *mut c_void) {
    if h.is_null() {
        return;
    }
    let _ = unsafe { Box::from_raw(h as *mut PluginInstance<Stub>) };
}

#[unsafe(no_mangle)]
pub extern "C" fn __rop_free_bytes(b: FfiBytes) {
    let _ = unsafe { b.reclaim() };
}

const VTABLE: FfiPluginVTable = FfiPluginVTable {
    abi_version: ABI_VERSION,
    module_version: 0,
    supports_periodic: 0,
    module_template: __rop_module_template,
    prepare: __rop_prepare,
    update_validated_parameters: __rop_update,
    process: __rop_process,
    set_ports: __rop_set_ports,
    periodic_update: __rop_periodic,
    drop: __rop_drop,
    free_bytes: __rop_free_bytes,
};

static VTABLES: [FfiPluginVTable; 1] = [VTABLE];

#[unsafe(no_mangle)]
pub extern "C" fn patches_plugin_init() -> FfiPluginManifest {
    FfiPluginManifest {
        abi_version: ABI_VERSION,
        count: 1,
        vtables: VTABLES.as_ptr(),
    }
}

#[unsafe(export_name = "patches_plugin_descriptor_hash_ReleaseOnUpdate")]
pub extern "C" fn __hash() -> u64 {
    descriptor_hash(&TEMPLATE.build_channels(patches_core::ModuleShape::default().channels as u32))
}
