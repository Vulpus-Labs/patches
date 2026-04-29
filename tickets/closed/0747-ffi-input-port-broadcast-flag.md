---
id: "0747"
title: Carry `broadcast_from_mono` across the FFI port-frame ABI
priority: high
created: 2026-04-29
adrs: ["0059", "0045"]
depends_on: []
---

## Summary

`FfiInputPort` (`patches-ffi-common/src/types.rs`) does not carry the
`broadcast_from_mono` flag declared on `patches_core::StereoInput`.
Both `From<&InputPort> for FfiInputPort` and the inverse drop the flag:
serialization omits it, deserialization hard-codes `false`. Host-side
graph validation (ADR 0059) marks mono→stereo cables with
`broadcast_from_mono = true` so `CablePool::read_stereo` knows to
splay the mono sample across both lanes; FFI-loaded modules
(`patches-vintage::VChorus` and any other plugin with a stereo input)
never see that flag, so on the first tick they hit the
`debug_assert!(false, "CablePool::read_stereo encountered a Mono cable
without broadcast — graph validation should prevent this")` at
`patches-core/src/cable_pool.rs:179`. The panic crosses
`extern "C" fn process` → `panic_cannot_unwind` → SIGABRT, taking
the audio thread (and the test process) with it.

Reproducer: `cargo test -p patches-integration-tests --test alloc_trap
audio_tick_no_alloc_poly_synth`. The `examples/poly_synth.patches`
graph wires `limit.out` (mono) into `chorus.in` (VChorus stereo);
broadcast is computed correctly in
`patches-planner::state::graph_index::build_input_buffer_map`, then
silently lost when the planner ships the resulting `InputPort::Stereo`
across the ABI to the plugin.

## Acceptance criteria

- [ ] `FfiInputPort` gains a `broadcast: u8` field (or equivalent
      packed encoding) covering the `Stereo` case; layout-stability
      assertions in `types.rs` updated.
- [ ] `From<&patches_core::InputPort> for FfiInputPort` propagates
      `s.broadcast_from_mono` for the `Stereo` variant; non-stereo
      variants serialize it as `0`.
- [ ] `From<FfiInputPort> for patches_core::InputPort` reconstitutes
      `broadcast_from_mono` from the new field instead of hard-coding
      `false`.
- [ ] `PortFrameLayout` / `pack_port_frame` / `PortView` continue to
      round-trip via `port_frame::tests`; add an explicit
      `Stereo+broadcast=true` round-trip test that fails today.
- [ ] Plugin SDK (`patches-ffi-common::sdk`) needs no caller change —
      conversion happens inside `From<FfiInputPort>`, so set_ports
      glue keeps working without code edits in plugins.
- [ ] `cargo test -p patches-integration-tests --test alloc_trap` passes
      again on `poly_synth.patches`. `vintage_baseline_matches_golden`
      and `vintage_synth_demo_compiles` also recover (same root cause).

## Notes

`FfiInputPort` is `#[repr(C)]` and carries an explicit byte layout;
adding a field is an ABI-breaking change, so existing plugin builds
will mismatch the host. Two options:

1. Bump the host/plugin handshake hash so stale plugin binaries are
   rejected at load — simplest, matches how `patches-ffi` already
   detects vtable drift (see `ffi_hash_mismatch.rs`).
2. Tag the new field behind a version byte in `PortFrameLayout` —
   allows old plugins to keep loading with `broadcast=false`, which
   is exactly the broken state we're trying to leave. Reject this
   option.

Go with (1). All in-tree plugins (`test-plugins/*`,
`patches-vintage`) rebuild against the new ABI in the same commit;
no external plugin ecosystem to coordinate with yet.

`FfiOutputPort` does not need the flag — broadcast is a destination-
side property of the cable read, not the source.
