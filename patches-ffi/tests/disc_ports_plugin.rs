//! Regression test for the FFI sink-translation path (ticket 0870).
//!
//! The DiscPorts plugin exposes a mono input and a poly output. With
//! disconnected ports the loader must route the mono read to the
//! permanent-zero sink and the poly write to the write-sink — both
//! living above the backplane in the host's index space and at the
//! plugin-relative `[0, 4)` range after the BACKPLANE_SIZE shift.

use std::path::PathBuf;

use patches_core::cable_pool::CablePool;
use patches_core::cables::{
    CableValue, InputPort, MonoInput, OutputPort, PolyOutput, MONO_READ_SINK,
    POLY_READ_SINK, MONO_WRITE_SINK, POLY_WRITE_SINK, RESERVED_SLOTS,
};
use patches_core::modules::{InstanceId, ModuleShape, ParameterMap, StructuralParams};
use patches_core::AudioEnvironment;
use patches_ffi::loader::load_plugin;
use patches_registry::ModuleBuilder;

fn disc_ports_dylib_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.push("target");
    path.push("debug");
    #[cfg(target_os = "macos")]
    path.push("libtest_disc_ports_plugin.dylib");
    #[cfg(target_os = "linux")]
    path.push("libtest_disc_ports_plugin.so");
    #[cfg(target_os = "windows")]
    path.push("test_disc_ports_plugin.dll");
    path
}

fn default_env() -> AudioEnvironment {
    AudioEnvironment {
        sample_rate: 48000.0,
        poly_voices: 16,
        periodic_update_interval: 32,
        hosted: false,
    }
}

#[test]
fn disconnected_ports_route_to_sinks_through_backplane_shift() {
    let mut builders = load_plugin(&disc_ports_dylib_path())
        .expect("failed to load disc-ports plugin");
    let builder = builders.remove(0);
    let env = default_env();
    let shape = ModuleShape { channels: 1 };
    let params = ParameterMap::new();
    let mut module = builder
        .build(&env, &shape, &params, &StructuralParams::new(), InstanceId::next())
        .expect("build failed");

    // Disconnected defaults: input → MONO_READ_SINK, output → POLY_WRITE_SINK.
    // These constants live in the host's index space; the loader's
    // pack_ports_into shifts them into plugin-relative space.
    let inputs = vec![InputPort::Mono(MonoInput::default())];
    let outputs = vec![OutputPort::Poly(PolyOutput::default())];
    module.set_ports(&inputs, &outputs);

    // Seed the read sinks with non-zero values to confirm the plugin
    // reads them as zero (the sink contract is "always zero", but we
    // want to verify the index translation, not the sink invariant).
    // Use a fresh scratch sized to the reserved range plus a few dyn
    // slots; the plugin reads MONO_READ_SINK and writes POLY_WRITE_SINK,
    // neither of which the test mutates.
    let mut scratch: Vec<CableValue> = vec![CableValue::mono(0.0); RESERVED_SLOTS];
    // Mark every non-sink slot with a sentinel so an off-by-one in the
    // shift would surface as a non-zero read.
    for (i, slot) in scratch.iter_mut().enumerate() {
        if i != MONO_READ_SINK && i != POLY_READ_SINK
            && i != MONO_WRITE_SINK && i != POLY_WRITE_SINK
        {
            *slot = CableValue::mono(1.0);
        }
    }
    let mut cycle: Vec<[CableValue; 2]> = vec![[CableValue::mono(0.0); 2]; 4];
    {
        let mut cp = CablePool::new(&mut scratch, &mut cycle, 0);
        module.process(&mut cp);
    }

    // The plugin wrote 0.25 into its (disconnected) poly output, which
    // routes to POLY_WRITE_SINK on the host. Verify the sentinel cells
    // are untouched — the shift must not have aimed the write at a live
    // backplane slot. Also verify the plugin's snapshot of the
    // MONO_READ_SINK read is zero: a misaligned shift would have
    // sampled a sentinel and the read would surface as 1.0 in the
    // dylib's exported atomic. We fetch that symbol via libloading
    // rather than linking it statically.
    use libloading::{Library, Symbol};
    let read = unsafe {
        let lib = Library::new(disc_ports_dylib_path()).expect("re-open plugin");
        let sym: Symbol<unsafe extern "C" fn() -> f32> =
            lib.get(b"disc_ports_last_input").expect("symbol lookup");
        sym()
    };
    assert_eq!(
        read, 0.0,
        "disconnected mono input must resolve to MONO_READ_SINK (0.0); got {read}",
    );

    for (i, slot) in scratch.iter().enumerate() {
        if i == MONO_READ_SINK || i == POLY_READ_SINK
            || i == MONO_WRITE_SINK || i == POLY_WRITE_SINK
        {
            continue;
        }
        assert_eq!(
            slot.as_mono(),
            1.0,
            "scratch slot {i} disturbed by disconnected write",
        );
    }
}
