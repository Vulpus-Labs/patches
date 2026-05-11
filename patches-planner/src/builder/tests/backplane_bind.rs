//! Ticket 0870: the planner refuses to wire any FFI plugin port to a
//! backplane scratch slot.

use patches_core::cables::{
    BACKPLANE_SIZE, AUDIO_OUT_L, GLOBAL_MIDI, MONO_READ_SINK, SCRATCH_CAPACITY,
};
use patches_core::{InputPort, MonoInput, MonoOutput, OutputPort, PolyInput, PolyOutput};

use super::super::{check_ffi_port_offsets, BuildErrorKind};

fn mono_in(idx: usize) -> InputPort {
    InputPort::Mono(MonoInput {
        cable_idx: idx, scale: 1.0, offset: 0.0, clip: None, connected: true, fused: false,
    })
}
fn poly_in(idx: usize) -> InputPort {
    InputPort::Poly(PolyInput {
        cable_idx: idx, scale: 1.0, offset: 0.0, clip: None, connected: true, fused: false,
    })
}
fn mono_out(idx: usize) -> OutputPort {
    OutputPort::Mono(MonoOutput { cable_idx: idx, connected: true })
}
fn poly_out(idx: usize) -> OutputPort {
    OutputPort::Poly(PolyOutput { cable_idx: idx, connected: true })
}

#[test]
fn builtin_offset_zero_short_circuits() {
    // Built-in modules pass scratch_base_offset = 0; any cable_idx
    // (including the live backplane) is fine.
    let inputs = vec![mono_in(AUDIO_OUT_L)];
    let outputs = vec![mono_out(GLOBAL_MIDI)];
    let r = check_ffi_port_offsets("BuiltinX", 0, &inputs, &outputs);
    assert!(r.is_ok());
}

#[test]
fn ffi_input_bound_to_backplane_is_rejected() {
    let inputs = vec![mono_in(AUDIO_OUT_L)];
    let outputs: Vec<OutputPort> = vec![];
    let err = check_ffi_port_offsets("DylibX", BACKPLANE_SIZE, &inputs, &outputs).unwrap_err();
    match err {
        BuildErrorKind::BackplaneBoundFfiPort { module_name, port_kind, cable_idx, .. } => {
            assert_eq!(module_name, "DylibX");
            assert_eq!(port_kind, "input");
            assert_eq!(cable_idx, AUDIO_OUT_L);
        }
        other => panic!("expected BackplaneBoundFfiPort, got {other:?}"),
    }
}

#[test]
fn ffi_output_bound_to_backplane_is_rejected() {
    let inputs: Vec<InputPort> = vec![];
    let outputs = vec![poly_out(GLOBAL_MIDI)];
    let err = check_ffi_port_offsets("DylibX", BACKPLANE_SIZE, &inputs, &outputs).unwrap_err();
    match err {
        BuildErrorKind::BackplaneBoundFfiPort { port_kind, cable_idx, .. } => {
            assert_eq!(port_kind, "output");
            assert_eq!(cable_idx, GLOBAL_MIDI);
        }
        other => panic!("expected BackplaneBoundFfiPort, got {other:?}"),
    }
}

#[test]
fn ffi_port_at_sink_slot_is_accepted() {
    // Sinks sit at `[RESERVED_SLOTS - SINK_SLOTS, RESERVED_SLOTS)`, just
    // past the backplane. Disconnected ports point here; they must
    // survive the check.
    let inputs = vec![mono_in(MONO_READ_SINK)];
    let outputs = vec![poly_out(MONO_READ_SINK + 1)];
    let r = check_ffi_port_offsets("DylibX", BACKPLANE_SIZE, &inputs, &outputs);
    assert!(r.is_ok(), "sink slot must be accepted, got {r:?}");
}

#[test]
fn ffi_port_in_cycle_region_is_accepted() {
    // Cycle indices (>= SCRATCH_CAPACITY) bypass the offset check;
    // the loader does not translate them.
    let inputs = vec![poly_in(SCRATCH_CAPACITY)];
    let outputs = vec![mono_out(SCRATCH_CAPACITY + 5)];
    let r = check_ffi_port_offsets("DylibX", BACKPLANE_SIZE, &inputs, &outputs);
    assert!(r.is_ok());
}
