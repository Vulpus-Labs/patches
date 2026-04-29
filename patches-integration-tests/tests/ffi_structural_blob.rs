//! Ticket 0739 — structural-blob round-trip across the FFI.
//!
//! Loads `test-structural-string-plugin`, packs a structural blob with
//! a non-empty `gain_path`, calls `prepare` directly through the
//! plugin's vtable, and asserts that:
//!
//! 1. `prepare` returns `PREPARE_OK`.
//! 2. The plugin's debug accessors expose the same path string and the
//!    gain it derived from the JSON sidecar at that path.
//! 3. An empty blob with no override yields the descriptor's default
//!    (empty path → gain 1.0).

use std::io::Write;

use patches_core::modules::{InstanceId, ModuleShape, StructuralParams, StructuralValue};
use patches_core::AudioEnvironment;
use patches_ffi::loader::load_plugin;
use patches_ffi_common::json;
use patches_ffi_common::structural_frame::pack_structural;
use patches_ffi_common::types::{FfiAudioEnvironment, FfiBytes, PREPARE_OK};
use patches_integration_tests::dylib_path;

fn write_sidecar(gain: f32) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "patches_0739_gain_{}_{}.json",
        std::process::id(),
        gain.to_bits()
    ));
    let mut f = std::fs::File::create(&path).expect("create sidecar");
    write!(f, "{{\"gain\": {gain}}}").unwrap();
    path
}

fn ffi_env() -> FfiAudioEnvironment {
    FfiAudioEnvironment::from(&AudioEnvironment {
        sample_rate: 48_000.0,
        poly_voices: 1,
        periodic_update_interval: 64,
        hosted: false,
    })
}

#[test]
fn structural_string_round_trip() {
    let path = dylib_path("test-structural-string-plugin");
    let builders = load_plugin(&path).expect("load_plugin");
    assert_eq!(builders.len(), 1);
    let vtable = *builders[0].vtable();

    // Read descriptor through the vtable and pack a blob with our path.
    let shape = ModuleShape::default();
    let descriptor = unsafe {
        let bytes = (vtable.describe)((&shape).into());
        let slice = bytes.as_slice();
        let desc = json::deserialize_module_descriptor(slice).unwrap();
        (vtable.free_bytes)(bytes);
        desc
    };
    assert_eq!(descriptor.module_name, "StructuralStringGain");
    assert_eq!(descriptor.structural_params.len(), 1);
    assert_eq!(descriptor.structural_params[0].name, "gain_path");

    let sidecar = write_sidecar(0.375);
    let mut params = StructuralParams::new();
    params.insert(
        "gain_path",
        0,
        StructuralValue::String(sidecar.to_string_lossy().into()),
    );
    let blob = pack_structural(&descriptor, &params);

    let desc_json = json::serialize_module_descriptor(&descriptor);

    let mut handle: *mut std::ffi::c_void = std::ptr::null_mut();
    let mut err = FfiBytes::empty();
    let status = unsafe {
        (vtable.prepare)(
            desc_json.as_ptr(),
            desc_json.len(),
            ffi_env(),
            InstanceId::next().as_u64(),
            blob.as_ptr(),
            blob.len(),
            &mut handle,
            &mut err,
        )
    };
    assert_eq!(status, PREPARE_OK, "prepare must succeed");
    assert!(!handle.is_null());

    // Inspect debug accessors.
    let lib = unsafe { libloading::Library::new(&path) }.unwrap();
    unsafe {
        let last_gain: libloading::Symbol<unsafe extern "C" fn() -> f32> =
            lib.get(b"structural_string_last_gain").unwrap();
        assert!(
            (last_gain() - 0.375).abs() < 1e-6,
            "gain not threaded through structural blob"
        );

        let last_path: libloading::Symbol<unsafe extern "C" fn() -> FfiBytes> =
            lib.get(b"structural_string_last_path").unwrap();
        let bytes = last_path();
        let s = std::str::from_utf8(bytes.as_slice()).unwrap().to_string();
        (vtable.free_bytes)(bytes);
        assert_eq!(s, sidecar.to_string_lossy());
    }

    unsafe { (vtable.drop)(handle) };
    let _ = std::fs::remove_file(&sidecar);
}

#[test]
fn structural_default_when_blob_uses_descriptor_default() {
    let path = dylib_path("test-structural-string-plugin");
    let builders = load_plugin(&path).expect("load_plugin");
    let vtable = *builders[0].vtable();

    let shape = ModuleShape::default();
    let descriptor = unsafe {
        let bytes = (vtable.describe)((&shape).into());
        let desc = json::deserialize_module_descriptor(bytes.as_slice()).unwrap();
        (vtable.free_bytes)(bytes);
        desc
    };

    // No override → descriptor default for File is empty string.
    let blob = pack_structural(&descriptor, &StructuralParams::new());
    let desc_json = json::serialize_module_descriptor(&descriptor);

    let mut handle: *mut std::ffi::c_void = std::ptr::null_mut();
    let mut err = FfiBytes::empty();
    let status = unsafe {
        (vtable.prepare)(
            desc_json.as_ptr(),
            desc_json.len(),
            ffi_env(),
            InstanceId::next().as_u64(),
            blob.as_ptr(),
            blob.len(),
            &mut handle,
            &mut err,
        )
    };
    assert_eq!(status, PREPARE_OK);
    assert!(!handle.is_null());

    let lib = unsafe { libloading::Library::new(&path) }.unwrap();
    unsafe {
        let last_gain: libloading::Symbol<unsafe extern "C" fn() -> f32> =
            lib.get(b"structural_string_last_gain").unwrap();
        assert!((last_gain() - 1.0).abs() < 1e-6);
        let last_path: libloading::Symbol<unsafe extern "C" fn() -> FfiBytes> =
            lib.get(b"structural_string_last_path").unwrap();
        let bytes = last_path();
        assert!(bytes.as_slice().is_empty());
        (vtable.free_bytes)(bytes);
    }

    unsafe { (vtable.drop)(handle) };
}
