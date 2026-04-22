//! E107 ticket 0625 — host ArcTable drains to zero after a plugin
//! release through real `extern "C"`.

use std::sync::{Arc, Mutex, OnceLock};

use patches_core::modules::{InstanceId, ModuleShape, ParameterMap};
use patches_core::param_frame::{pack_into, ParamFrame, ParamView, ParamViewIndex};
use patches_core::param_layout::{compute_layout, defaults_from_descriptor};
use patches_core::AudioEnvironment;
use patches_ffi::loader::load_plugin;
use patches_ffi_common::abi::{Handle, HostEnv};
use patches_ffi_common::arc_table::{RuntimeArcTables, RuntimeArcTablesConfig};
use patches_ffi_common::json;
use patches_ffi_common::types::FfiAudioEnvironment;
use patches_integration_tests::dylib_path;

// Static bridge so the `extern "C"` release trampoline can find the
// per-test `ArcTableAudio` half.
pub(crate) static AUDIO: OnceLock<Mutex<Option<patches_ffi_common::arc_table::RuntimeAudioHandles>>> =
    OnceLock::new();

pub(crate) extern "C" fn release_trampoline(id: u64) {
    let mut guard = AUDIO.get().expect("audio half not installed").lock().unwrap();
    if let Some(h) = guard.as_mut() {
        h.float_buffers.release(id);
    }
}

extern "C" fn song_release_noop(_: u64) {}

fn host_env() -> HostEnv {
    HostEnv {
        float_buffer_release: release_trampoline,
        song_data_release: song_release_noop,
    }
}

#[test]
fn arctable_drains_after_plugin_release() {
    let path = dylib_path("test-release-on-update-plugin");
    let builders = load_plugin(&path).expect("load release-on-update");
    let vtable = *builders[0].vtable();

    // Wire host ArcTable.
    let (mut control, audio) = RuntimeArcTables::new(RuntimeArcTablesConfig {
        float_buffers: 4,
    });
    let _ = AUDIO.set(Mutex::new(Some(audio)));

    // Mint one Arc<[f32]> payload; keep a weak-ish check via a wrapper
    // around DropThreadRecorder packaged as Arc<[f32]> — we can't use
    // a custom struct because the table is typed to [f32]. Use a
    // pre-dropped sentinel via a side-channel instead: mint an Arc<[f32]>
    // whose strong-count we observe post-drain.
    let payload: Arc<[f32]> = Arc::from(vec![1.0f32, 2.0, 3.0].into_boxed_slice());
    let strong_before = Arc::strong_count(&payload);
    assert_eq!(strong_before, 1);
    let id = control.mint_float_buffer(Arc::clone(&payload)).unwrap();
    assert_eq!(control.float_buffer_live_count(), 1);

    // Drive a prepare + update(id) → plugin calls env.float_buffer_release
    // through the trampoline, which pushes onto the release queue.
    let shape = ModuleShape::default();
    let descriptor_json_bytes = unsafe {
        let b = (vtable.describe)((&shape).into());
        let slice = b.as_slice();
        let d = json::deserialize_module_descriptor(slice).unwrap();
        (vtable.free_bytes)(b);
        d
    };
    let env = AudioEnvironment {
        sample_rate: 48_000.0,
        poly_voices: 1,
        periodic_update_interval: 64,
        hosted: false,
    };
    let ffi_env = FfiAudioEnvironment::from(&env);
    let ser = json::serialize_module_descriptor(&descriptor_json_bytes);
    let handle = unsafe {
        (vtable.prepare)(ser.as_ptr(), ser.len(), ffi_env, InstanceId::next().as_u64())
    };
    assert!(!handle.is_null());

    let layout = compute_layout(&descriptor_json_bytes);
    let defaults = defaults_from_descriptor(&descriptor_json_bytes);
    let index = ParamViewIndex::from_layout(&layout);
    let mut frame = ParamFrame::with_layout(&layout);
    pack_into(&layout, &defaults, &ParameterMap::new(), &mut frame).unwrap();
    frame.buffer_slots_mut()[0] = id.as_u64();

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

    // Drain: the plugin's release should have pushed our id.
    control.drain_released();
    assert_eq!(
        control.float_buffer_live_count(),
        0,
        "ArcTable must drain to zero after release"
    );
    assert_eq!(Arc::strong_count(&payload), 1, "host holds the sole Arc");

    unsafe { (vtable.drop)(handle) };
    drop(control);

    // Reset audio half so other tests start clean.
    *AUDIO.get().unwrap().lock().unwrap() = None;
}
