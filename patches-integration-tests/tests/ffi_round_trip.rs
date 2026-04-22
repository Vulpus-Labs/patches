//! E107 ticket 0621 — FFI round-trip parity.
//!
//! Load the `all-tags` dylib, build a ParamFrame with every ScalarTag
//! plus a buffer id, dispatch through real `extern "C"`, read the
//! plugin's recorded state via exported debug accessors, assert parity.

use patches_core::modules::{InstanceId, ModuleShape, ParameterMap, ParameterValue};
use patches_core::param_frame::{pack_into, ParamFrame, ParamView, ParamViewIndex};
use patches_core::param_layout::{compute_layout, defaults_from_descriptor};
use patches_core::AudioEnvironment;
use patches_ffi::loader::load_plugin;
use patches_ffi_common::abi::{Handle, HostEnv};
use patches_ffi_common::types::FfiAudioEnvironment;
use patches_ffi_common::json;
use patches_integration_tests::dylib_path;

extern "C" fn noop_release(_: u64) {}

fn host_env() -> HostEnv {
    HostEnv {
        float_buffer_release: noop_release,
        song_data_release: noop_release,
    }
}

#[test]
fn all_scalar_tags_round_trip() {
    let path = dylib_path("test-all-tags-plugin");
    let builders = load_plugin(&path).expect("load_plugin");
    assert_eq!(builders.len(), 1);
    let vtable = *builders[0].vtable();

    let shape = ModuleShape::default();
    let descriptor_json = unsafe {
        let bytes = (vtable.describe)((&shape).into());
        let slice = bytes.as_slice();
        let desc = json::deserialize_module_descriptor(slice).unwrap();
        (vtable.free_bytes)(bytes);
        desc
    };
    assert_eq!(descriptor_json.module_name, "AllTags");

    // prepare
    let env = AudioEnvironment {
        sample_rate: 48_000.0,
        poly_voices: 1,
        periodic_update_interval: 64,
        hosted: false,
    };
    let ffi_env = FfiAudioEnvironment::from(&env);
    let desc_bytes = json::serialize_module_descriptor(&descriptor_json);
    let handle = unsafe {
        (vtable.prepare)(
            desc_bytes.as_ptr(),
            desc_bytes.len(),
            ffi_env,
            InstanceId::next().as_u64(),
        )
    };
    assert!(!handle.is_null());

    // Build a ParamFrame with specific values for every tag.
    let layout = compute_layout(&descriptor_json);
    let defaults = defaults_from_descriptor(&descriptor_json);
    let mut params = ParameterMap::new();
    params.insert_param("g", 0, ParameterValue::Float(0.321));
    params.insert_param("n", 0, ParameterValue::Int(-42));
    params.insert_param("b", 0, ParameterValue::Bool(true));
    params.insert_param("m", 0, ParameterValue::Enum(2));
    let mut frame = ParamFrame::with_layout(&layout);
    pack_into(&layout, &defaults, &params, &mut frame).unwrap();
    frame.buffer_slots_mut()[0] = 0x1234_5678_9ABC_DEF0;

    let index = ParamViewIndex::from_layout(&layout);
    let view = ParamView::new(&index, &frame);
    let bytes = view.wire_bytes();

    let env_v = host_env();
    unsafe {
        (vtable.update_validated_parameters)(
            handle as Handle,
            bytes.as_ptr(),
            bytes.len(),
            &env_v,
        );
    }

    // Read back through exported debug accessors.
    let lib = unsafe { libloading::Library::new(&path) }.unwrap();
    unsafe {
        let f: libloading::Symbol<unsafe extern "C" fn() -> f32> =
            lib.get(b"all_tags_last_float").unwrap();
        assert_eq!(f(), 0.321);
        let i: libloading::Symbol<unsafe extern "C" fn() -> i64> =
            lib.get(b"all_tags_last_int").unwrap();
        assert_eq!(i(), -42);
        let b: libloading::Symbol<unsafe extern "C" fn() -> bool> =
            lib.get(b"all_tags_last_bool").unwrap();
        assert!(b());
        let e: libloading::Symbol<unsafe extern "C" fn() -> u32> =
            lib.get(b"all_tags_last_enum").unwrap();
        assert_eq!(e(), 2);
        let buf: libloading::Symbol<unsafe extern "C" fn() -> u64> =
            lib.get(b"all_tags_last_buffer").unwrap();
        assert_eq!(buf(), 0x1234_5678_9ABC_DEF0);
    }

    unsafe { (vtable.drop)(handle) };
}
