---
id: "0872"
title: Document the wire packing rules (ParamFrame, PortFrame, structural blob, CableValue)
priority: medium
created: 2026-05-11
epic: E145
---

## Summary

Four binary wire formats cross the FFI:

1. **`ParamFrame` scalar area.** Realtime parameter values, packed
   per [patches-core/src/param_layout/mod.rs:80-108](patches-core/src/param_layout/mod.rs#L80-L108):
   sort scalars by `(name, index)`, greedy-pack with natural
   alignment, pad total to max-align. Both host and plugin recompute
   the layout independently from the descriptor; if the algorithm
   ever changes silently, every external plugin breaks (caught by
   `descriptor_hash`, but only at load).
2. **`PortFrame`.** Per-instance port wiring, layout in
   [patches-ffi-common/src/port_frame.rs](patches-ffi-common/src/port_frame.rs):
   `PortFrameHeader` (`#[repr(C)]`, idx + counts) followed by
   typed arrays of `FfiInputPort` / `FfiOutputPort` with computed
   alignment offsets.
3. **Structural blob.** Construction-time parameter values, format
   already documented inline at
   [patches-ffi-common/src/structural_frame.rs:7-22](patches-ffi-common/src/structural_frame.rs#L7-L22):
   `[u16 slot_count] [u8 tag] [u32 value_len] [bytes]…` with four
   tags (BOOL/I64/F64/STRING). Copy out and formalise.
4. **`CableValue` and cycle slot.** `#[repr(transparent)]` over
   `[f32; 16]` ([patches-core/src/cables/mod.rs:285-287](patches-core/src/cables/mod.rs#L285-L287)):
   64 bytes, alignment 4, no padding, no tag. Cycle slot is
   `[CableValue; 2]` = 128 bytes ping-pong; consumer reads slot
   `1 - write_index`, producer writes slot `write_index` (or
   current-tick output if the cable is in a fused acyclic region —
   see ADR 0072 phase 2 and `FfiInputPort.fused`).

This ticket produces a single packing-formats doc, paired with the
schema doc from 0871, that together let an external SDK author
implement the host contract from the spec alone.

## Acceptance criteria

- [ ] New page under [docs/src/](docs/src/) (suggested
      `docs/src/abi/wire-formats.md`) covering all four formats.
- [ ] **ParamFrame section** spells out:
  - Sort key: `(name: &str, index: u16)`, lexicographic on name then
    numeric on index.
  - Per-`ScalarTag` size and alignment (Float/Enum: 4/4, Int: 8/8,
    Bool: 1/1) — quote the table from
    [patches-core/src/param_layout/mod.rs:33-49](patches-core/src/param_layout/mod.rs#L33-L49).
  - Greedy align-up packing rule.
  - Final `scalar_size` rounded up to the max alignment observed.
  - Wire bytes are the scalar area padded to a multiple of 8 bytes
    (`U64_SIZE`, see
    [patches-core/src/param_frame/view.rs:159-183](patches-core/src/param_frame/view.rs#L159-L183));
    8-byte alignment requirement on the buffer.
- [ ] **PortFrame section** documents the C layout:
  `[PortFrameHeader] [FfiInputPort × input_count] [FfiOutputPort × output_count]`
  with the alignment-padding rules from
  [patches-ffi-common/src/port_frame.rs:41-83](patches-ffi-common/src/port_frame.rs#L41-L83).
  Quote the `#[repr(C)]` struct definitions verbatim. Document the
  port-tag values (`PORT_TAG_MONO=0`, `PORT_TAG_POLY=1`,
  `PORT_TAG_STEREO=2`) and the per-port flags (`connected`,
  `broadcast`, `fused`).
- [ ] **Structural blob section** carries the full grammar from
  [patches-ffi-common/src/structural_frame.rs:7-22](patches-ffi-common/src/structural_frame.rs#L7-L22)
  with worked encoding examples for each tag.
- [ ] **CableValue section**: `struct { float lanes[16]; }`, 64
  bytes, alignment 4. Lane semantics (lane 0 = mono, lanes 0-1 =
  stereo, all 16 = poly) cross-referenced to the descriptor's
  port kind. Cycle slot is `[CableValue; 2]` = 128 bytes; ping-pong
  semantics with `write_index`. Note the fused-cable carve-out
  (ADR 0072 phase 2).
- [ ] Stability statement: changes to any of these formats require
  an ABI bump. The `descriptor_hash` only catches *descriptor-level*
  drift, not packing-algorithm drift; the packing algorithm IS the
  contract, and changing it without a bump silently corrupts every
  plugin.
- [ ] Linked from manual TOC; cross-linked with the descriptor
  schema doc (0871).
- [ ] Link from [patches-ffi-common/src/lib.rs](patches-ffi-common/src/lib.rs)
  module-level doc comment back to the manual page.
- [ ] `just push` clean.

## Notes

ParamView's perfect-hash index
([patches-core/src/param_frame/view.rs:36-95](patches-core/src/param_frame/view.rs#L36-L95))
is host-internal — plugin SDK doesn't need to know. The doc covers
only the *byte layout* of the scalar area, which is what the
plugin actually decodes.

`FfiBytes` allocation contract (plugin allocates, plugin frees via
`vtable.free_bytes`) belongs in the vtable doc rather than the wire
formats doc — the bytes are opaque to the host once received. Note
this with a cross-reference; don't duplicate.

The `patches_plugin_descriptor_hash_<name>` symbol convention and
the FNV-1a hash algorithm (param_layout/hash.rs) are also part of
the load-time contract. Could go in this doc or a third "load
protocol" doc; pick one. If a third doc materialises, add a follow-
up ticket; do not expand scope here.
