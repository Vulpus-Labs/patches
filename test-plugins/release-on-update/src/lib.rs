//! Fixture plugin that calls `env.float_buffer_release(id)` on every
//! `update_validated_parameters` invocation. Used for:
//! - E107 ticket 0624: call twice → host ArcTable audit trips.
//! - E107 ticket 0625: call once → host ArcTable drains to zero.
//!
//! Hand-written ABI surface so we can reach `HostEnv` from
//! `update_validated_parameters` — the stock `export_plugin!` macro
//! drops the env argument.

use std::ffi::c_void;

use patches_core::cables::CableValue;
use patches_core::modules::{ModuleDescriptor, ModuleShape};
use patches_core::param_frame::ParamViewIndex;
use patches_core::param_layout::compute_layout;
use patches_ffi_common::abi::{Handle, HostEnv};
use patches_ffi_common::port_frame::PortLayout;
use patches_ffi_common::sdk::{decode_param_frame, PluginInstance};
use patches_ffi_common::types::{
    FfiAudioEnvironment, FfiBytes, FfiModuleShape, FfiPluginManifest, FfiPluginVTable,
    ABI_VERSION,
};
use patches_ffi_common::{descriptor_hash, json};

pub struct Stub;

fn describe(shape: &ModuleShape) -> ModuleDescriptor {
    ModuleDescriptor::new("ReleaseOnUpdate", shape.clone())
        .file_param("s", &["wav"])
}

// Bare-bones Module impl; we only need prepare/drop + param_index layout.
impl patches_core::Module for Stub {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        describe(shape)
    }
    fn prepare(
        _env: &patches_core::AudioEnvironment,
        _d: ModuleDescriptor,
        _id: patches_core::modules::InstanceId,
    ) -> Self {
        Stub
    }
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
pub extern "C" fn __rop_describe(shape: FfiModuleShape) -> FfiBytes {
    let core_shape: ModuleShape = shape.into();
    FfiBytes::from_vec(json::serialize_module_descriptor(&describe(&core_shape)))
}

#[unsafe(no_mangle)]
pub extern "C" fn __rop_prepare(
    descriptor_json: *const u8,
    descriptor_json_len: usize,
    _env: FfiAudioEnvironment,
    _instance_id: u64,
) -> *mut c_void {
    let slice =
        unsafe { std::slice::from_raw_parts(descriptor_json, descriptor_json_len) };
    let descriptor = match json::deserialize_module_descriptor(slice) {
        Ok(d) => d,
        Err(_) => return std::ptr::null_mut(),
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
    Box::into_raw(inst) as *mut c_void
}

#[unsafe(no_mangle)]
pub extern "C" fn __rop_update(
    handle: Handle,
    bytes: *const u8,
    len: usize,
    env: *const HostEnv,
) {
    let inst = unsafe { &mut *(handle as *mut PluginInstance<Stub>) };
    let slice = unsafe { std::slice::from_raw_parts(bytes, len) };
    let view = match decode_param_frame(slice, &inst.param_index) {
        Ok(v) => v,
        Err(_) => return,
    };
    if let Some(id) = view.fetch_buffer_static("s", 0) {
        let env = unsafe { &*env };
        (env.float_buffer_release)(id.as_u64());
    }
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
    describe: __rop_describe,
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
    descriptor_hash(&describe(&ModuleShape::default()))
}
