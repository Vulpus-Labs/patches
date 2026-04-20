---
id: "E097"
title: ParamLayout as a pure function (ADR 0045 Spike 1)
created: 2026-04-19
depends_on: ["E096"]
tickets: ["0577", "0578", "0579"]
---

## Goal

Spike 1 of ADR 0045. Compute `ParamLayout` deterministically from
a `ModuleDescriptor`: produce `scalar_size`, an ordered
`ScalarSlot` table, a `BufferSlot` table, and a stable
`descriptor_hash`. No runtime behaviour changes — this spike lands
a pure function in `patches-ffi-common` with no dependents yet.

Spike 0 (E096, closed) retired `ParameterValue::String` and moved
`ParameterValue::Enum` to `u32`. Every parameter kind that must
appear in the packed scalar area is now `Copy` and has a fixed
wire size: `Float(f32)`, `Int(i64)`, `Bool(bool)`, `Enum(u32)`.
`File` / `FloatBuffer` become buffer slots, not scalars.
`ParameterKind::String` no longer exists, so `ParamLayout` need
not handle it.

After this epic:

- `patches-ffi-common::param_layout` exposes `ParamLayout`,
  `ScalarSlot`, `ScalarTag`, `BufferSlot`, and
  `compute_layout(&ModuleDescriptor) -> ParamLayout`.
- Layout is deterministic across runs and machines: same
  descriptor ⇒ byte-identical layout and hash.
- Scalar offsets respect natural alignment; `scalar_size` is the
  minimum size that covers every slot and is a multiple of the
  maximum scalar alignment.
- `descriptor_hash` is a stable 64-bit digest over descriptor
  shape (param names, kinds, variant lists, port counts) using a
  canonical byte encoding — independent of `HashMap` iteration
  order and `Hash` derive changes.
- No wiring into audio path, FFI codec, or module runtime.

## Tickets

| ID   | Title                                                            | Priority | Depends on |
| ---- | ---------------------------------------------------------------- | -------- | ---------- |
| 0577 | ParamLayout types and compute_layout in patches-ffi-common       | high     | —          |
| 0578 | Stable descriptor_hash with canonical byte encoding              | high     | 0577       |
| 0579 | Property tests: determinism, alignment, coverage                 | medium   | 0577, 0578 |

## Affected surface

- `patches-ffi-common`: new `param_layout` module. No changes to
  `json`, `types`.
- `patches-core`: read-only use of `ModuleDescriptor` and
  `ParameterKind`. No trait or type changes.
- Nothing in `patches-ffi`, `patches-engine`, modules, or hosts.

## Design notes

- **Ordering.** Slots sorted by `(parameter_name, indexed_position)`
  for determinism regardless of descriptor iteration order.
- **Scalar packing.** Greedy assignment with natural alignment of
  each slot's `ScalarTag`. No padding optimisation pass — deterministic
  ordering matters more than minimum size. `scalar_size` rounded
  up to max scalar alignment so arrays of scratch buffers align.
- **Buffer slots.** One `BufferSlot` per `File` / `FloatBuffer`
  parameter, indexed 0.. in the same canonical order. Tail slot
  layout is `slot_index * sizeof(u64)`.
- **Hashing.** SHA-256 truncated to `u64` over a canonical byte
  stream: little-endian lengths + UTF-8 names + kind tags + variant
  names. No `Hash` derive, no `HashMap` iteration. Tested on
  multiple host architectures via CI.

## Definition of done

- `cargo test -p patches-ffi-common` green, including new
  property tests for determinism / alignment / coverage.
- `cargo clippy` clean.
- No dependent crate imports `param_layout` yet — verified by
  inspection. Spike 3 will wire it.

## Out of scope

- `ParamFrame`, `pack_into`, `ParamView` (Spike 3). The SPSC-triplet
  shuttle originally planned alongside these was rolled back during
  E099 — see ADR 0045 §3 and ticket 0588 roll-back notes.
- `ArcTable` and refcount map (Spike 2).
- Audio-thread allocator trap (Spike 4).
- Any FFI ABI change or descriptor-hash check at load (Spike 7).
