---
id: "0739"
title: FFI — structural blob encoding and prepare entry-point extension
priority: medium
created: 2026-04-28
epic: "E126"
adrs: ["0060", "0045"]
depends_on: ["0734"]
---

## Summary

Extend the FFI to carry structural params as a positional packed
blob, mirroring `ParamFrame` for the realtime side. Absorb the blob
into the existing `prepare` ABI entry point — there is no separate
`apply_structural` call. The plugin owns its decoder for any
file-backed inputs.

## Acceptance criteria

- [ ] `prepare` ABI entry point gains `structural_blob_ptr` /
      `structural_blob_len` parameters and returns a status code with
      out-buffer for error string.
- [ ] Structural blob format defined and documented in
      `patches-ffi-common/src/structural_frame.rs` (new module):
      `[u16 slot_count] [u8 type_tag][u32 value_len][bytes]…` with
      slot order matching `descriptor.structural_params`. Type tags:
      `0=bool, 1=i64, 2=f64, 3=string`.
- [ ] Encode helper on the host side: `pack_structural(descriptor,
      &StructuralParams) -> Vec<u8>`.
- [ ] Decode helper exposed via `patches-ffi-common::sdk` for
      plugins: a typed-view wrapper analogous to `ParamView` but for
      structural slots, supporting string-typed reads.
- [ ] SDK macros that generate `__patches_prepare` updated to
      decode the structural blob and call the module's
      `Module::prepare` with a `StructuralParams` constructed from
      the decoded values.
- [ ] `test-plugins/`: extend an existing plugin (or add a small new
      one) with a structural string param to prove the round-trip.
      Either: (a) a minimal "file-backed gain" plugin that reads a
      gain factor from a JSON sidecar, or (b) a stub `ConvReverb`
      plugin demonstrating an `ir_path` structural param. (a) is
      cheaper and exercises the same ABI surface.
- [ ] Integration test under `patches-integration-tests/` builds the
      plugin and confirms structural params arrive intact across the
      ABI.
- [ ] `cargo test` and `cargo clippy` pass.

## Notes

Bump the plugin ABI version constant per ADR 0039 conventions; the
blob field is additive so old plugins refusing the new entry-point
arity must surface a clear version-mismatch error.
