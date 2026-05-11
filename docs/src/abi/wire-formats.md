# FFI wire formats

This page specifies the four binary wire formats that cross the Patches
host/plugin FFI boundary. It is the normative reference for SDK authors —
including out-of-tree SDKs in other languages — implementing the plugin side
of the contract. The Rust source is the canonical implementation; this page
is the spec.

The four formats are:

1. **`ParamFrame` scalar area** — realtime parameter values, audio thread.
2. **`PortFrame`** — per-instance port wiring, control thread.
3. **Structural blob** — construction-time parameter values, prepare time.
4. **`CableValue` and cycle slot** — cable-pool storage, audio thread.

The descriptor schema that names the parameters and ports referenced by
these frames is documented separately in
[descriptor schema](./descriptor-schema.md). The vtable through which the
host invokes the plugin (and the [`FfiBytes`] allocation contract) is
documented with the vtable; this page covers byte layout only.

## Conventions

- All multi-byte integers are **little-endian** on the wire. The host and
  the plugin compile for the same target architecture in practice;
  endianness is fixed to little-endian rather than “native” so the spec is
  unambiguous on first reading.
- All structures crossing the boundary are `#[repr(C)]` or
  `#[repr(transparent)]`. Field offsets, sizes, and alignments are computed
  by C ABI rules.
- Sizes given as `sizeof(T)` follow the host target's pointer width;
  `usize` is `4` bytes on 32-bit, `8` bytes on 64-bit.
- “Pad” means zero-fill; the values of padding bytes are unspecified for
  reads but every implementation in this repo writes zero.

## Stability

Changes to any wire format on this page require an **ABI version bump**
(`ABI_VERSION` in `patches-ffi-common`). The host refuses to load any
plugin whose `FfiPluginVTable::abi_version` does not match.

The per-module `descriptor_hash` ([load-time symbol](#load-time-descriptor-hash))
only catches *descriptor-level* drift: parameter shape, port shape, names,
kinds. It does **not** catch packing-algorithm drift. The packing algorithm
on this page **is the contract**; changing the sort key, the alignment
rule, the padding rule, or any tag value without bumping `ABI_VERSION`
silently corrupts every plugin built against the previous version.

---

## 1. `ParamFrame` scalar area

The realtime parameter values for one module instance, pushed across the
FFI on the audio thread via `FfiPluginVTable::update_validated_parameters`.

The host and the plugin each compute the layout independently from the
shared `ModuleDescriptor` (carried as JSON at `prepare`). The packing
algorithm below is what makes those two computations agree.

### Sort key

Parameters are sorted by **`(name, index)`**, lexicographic byte
comparison on `name` followed by numeric ascending comparison on
`index: u16`. Declaration order in the descriptor does **not** influence
the wire offset of any slot.

### Per-tag size and alignment

Each `realtime_param` carries a `ParameterKind`. For wire-layout purposes
it reduces to one of four `ScalarTag` values with the following fixed
size and alignment in bytes:

| Tag    | Size | Align | `ParameterKind` source                |
| ------ | ---: | ----: | ------------------------------------- |
| Float  |    4 |     4 | `Float`                               |
| Int    |    8 |     8 | `Int`, `SongName`                     |
| Bool   |    1 |     1 | `Bool`                                |
| Enum   |    4 |     4 | `Enum`                                |

`ParameterKind::File` is structural-only and never appears in the
realtime scalar area.

```rust
impl ScalarTag {
    pub const fn size(self) -> u32 {
        match self {
            ScalarTag::Float => 4,
            ScalarTag::Int   => 8,
            ScalarTag::Bool  => 1,
            ScalarTag::Enum  => 4,
        }
    }

    pub const fn align(self) -> u32 {
        match self {
            ScalarTag::Float => 4,
            ScalarTag::Int   => 8,
            ScalarTag::Bool  => 1,
            ScalarTag::Enum  => 4,
        }
    }
}
```

### Packing rule

Greedy align-up. Walk the sorted scalar list once; for each slot:

1. `offset := align_up(offset, tag.align())` where
   `align_up(n, a) = (n + a − 1) & !(a − 1)` and `a` is a power of two.
2. Record the slot at this `offset`.
3. `offset := offset + tag.size()`.

After the walk, the unpadded scalar area is `offset` bytes long. The
final `scalar_size` is rounded up to the maximum alignment observed
across all slots:

```text
scalar_size = align_up(offset, max_align_seen)
```

If there are no scalar slots, `scalar_size = 0`.

`scalar_size` is the value the plugin uses to validate frame length; it
is also the stride for arrays of frames if a host ever sends them. Always
rounding up to `max_align_seen` keeps that stride aligned.

### On-the-wire padding

The bytes pushed across the FFI are the scalar area padded up to a
multiple of `U64_SIZE = 8`:

```text
wire_len = ceil(scalar_size / 8) * 8
```

The buffer's start pointer is **8-byte aligned**. (In the reference host
implementation the storage is a `Vec<u64>`, which guarantees this for
free.) Bytes between `scalar_size` and `wire_len` are zero on send; the
plugin must not interpret them.

### Decoding

The plugin computes its own `ParamLayout` from the descriptor (using the
algorithm above) and reads each scalar at its computed offset. The
scalars are not aligned to their natural alignment on the wire (the
greedy packer only aligns to each slot's own align, not to anything
larger), so plugins must use unaligned loads.

The reference host builds a perfect-hash index over the layout so
named lookups are O(1) on the audio thread. That index is host-internal;
plugin SDKs that prefer byte-offset decoding (the “read the offset from
the layout, load at that offset” path) need not replicate it.

---

## 2. `PortFrame`

The per-instance port wiring, pushed across the FFI on the audio thread
via `FfiPluginVTable::set_ports`. Carries this module's input and output
ports — cable indices, scaling, connectivity, and (for stereo) the
mono-broadcast flag.

### Layout

The frame is a `#[repr(C)]` header followed by typed `#[repr(C)]`
arrays:

```text
[PortFrameHeader]
[align pad up to align_of<FfiInputPort>]
[FfiInputPort × input_count]
[align pad up to align_of<FfiOutputPort>]
[FfiOutputPort × output_count]
```

The starting offsets of the input and output arrays are derived as:

```rust
let header_size = size_of::<PortFrameHeader>();         // 12
let in_off  = align_up(header_size, align_of::<FfiInputPort>());
let in_len  = input_count * size_of::<FfiInputPort>();
let out_off = align_up(in_off + in_len, align_of::<FfiOutputPort>());
let total   = out_off + output_count * size_of::<FfiOutputPort>();
```

Padding bytes between header and inputs, and between inputs and outputs,
are zero on send. The frame's `total_size` is `out_off + output_count *
size_of::<FfiOutputPort>()` — there is no trailing pad.

The plugin computes the same layout from the descriptor's `input_count`
and `output_count` and decodes by offset.

### Header

```rust
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortFrameHeader {
    /// Module pool index for this frame.
    pub idx: u32,
    pub input_count: u32,
    pub output_count: u32,
}
```

`idx` is the host-side module pool index. The plugin treats it as opaque
metadata — it is forwarded back to the host in diagnostic paths but not
interpreted. `input_count` and `output_count` must match the descriptor;
the audio-thread reader's behaviour on mismatch is undefined (the host
rejects mismatches at pack time on the control thread).

### Port tags

```rust
pub const PORT_TAG_MONO:   u8 = 0;
pub const PORT_TAG_POLY:   u8 = 1;
pub const PORT_TAG_STEREO: u8 = 2;
```

A port's tag must agree with the descriptor's declared `CableKind` for
that port slot. Any other value is reserved.

### `FfiInputPort`

```rust
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FfiInputPort {
    pub tag: u8,
    pub cable_idx: usize,
    pub scale: f32,
    pub connected: u8,
    /// Stereo-only: when `tag == PORT_TAG_STEREO` and the cable is mono,
    /// the reader splays the mono sample across both lanes. Always `0`
    /// for non-stereo variants.
    pub broadcast: u8,
    /// ADR 0072 phase 2: when `1`, the cable lies in an acyclic region
    /// and the consumer reads the producer's current-tick output (slot
    /// `wi`) instead of the previous-tick output (slot `1 - wi`).
    pub fused: u8,
}
```

Flag fields are `0` or `1`. The host writes exactly those values; the
plugin must treat any non-zero value as `true` and zero as `false`.

| Flag        | Semantics                                                                 |
| ----------- | ------------------------------------------------------------------------- |
| `connected` | `1` if the port is wired to a producer; `0` if it resolves to a sink slot |
| `broadcast` | `1` on a stereo input fed by a mono cable: read lane 0 into both lanes    |
| `fused`     | `1` for an acyclic-region input: read producer's current-tick slot        |

### `FfiOutputPort`

```rust
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FfiOutputPort {
    pub tag: u8,
    pub cable_idx: usize,
    pub connected: u8,
}
```

`connected` is `1` if any reader is wired to this output, `0` if it
resolves to a write-sink slot. Outputs do not carry `broadcast` or
`fused`; both are properties of the consumer's read, not the producer's
write.

### Scratch / cycle index space and the backplane shift

`cable_idx` is interpreted against the cable pool:

| Range                                                    | Region   | Slot shape         |
| -------------------------------------------------------- | -------- | ------------------ |
| `[0, SCRATCH_CAPACITY)`                                  | scratch  | `CableValue`       |
| `[SCRATCH_CAPACITY, SCRATCH_CAPACITY + CYCLE_CAPACITY)`  | cycle    | `[CableValue; 2]`  |

In the reference host, `SCRATCH_CAPACITY = 2048` and
`CYCLE_CAPACITY = 128`. These values are exposed as `pub const`s in
`patches-core::cables` and shipped to the plugin via the descriptor
schema; plugins should not bake them at compile time.

The host applies a **scratch-base translation** before writing
`cable_idx` into the frame. For built-in modules the translation is the
identity. For FFI plugins, the host hides the backplane (sinks,
host-control rendezvous slots, reserved bus rendezvous) from plugin
view by subtracting `BACKPLANE_SIZE` from every scratch-region index:

```text
if cable_idx < SCRATCH_CAPACITY:
    on_wire = cable_idx - BACKPLANE_SIZE      // host's BACKPLANE_SIZE = RESERVED_SLOTS - SINK_SLOTS
else:
    on_wire = cable_idx                       // cycle indices pass through
```

`BACKPLANE_SIZE` is currently `12` in the reference host. The host
planner refuses to wire any plugin-visible port whose scratch
`cable_idx` is below `BACKPLANE_SIZE`; the subtraction therefore never
underflows. The plugin sees a flat scratch region starting at index `0`
(its first plugin-visible slot is what the host calls
`BACKPLANE_SIZE`). Cycle indices are not shifted.

The reference Rust SDK reconstructs a `CablePool` from the `process`
arguments and indexes by the on-wire value directly.

---

## 3. Structural blob

The construction-time parameter values for one module instance, passed to
`FfiPluginVTable::prepare` as `(structural_blob_ptr, structural_blob_len)`.
This is the analogue of `ParamFrame` for slots that never reach the audio
thread (file paths, song names, count-style integers that pin instance
shape).

The format is **positional**, mirroring the descriptor's
`structural_params` slot order. There is no per-slot name on the wire;
slot `i` in the blob corresponds to slot `i` in
`descriptor.structural_params`.

### Grammar

```text
blob       ::= u16-le slot_count slot{slot_count}
slot       ::= u8 tag  u32-le value_len  byte{value_len}

tag = 0 -> bool   (value_len = 1; byte 0 = false, byte 1 = true)
      1 -> i64    (value_len = 8; two's-complement little-endian)
      2 -> f64    (value_len = 8; IEEE-754 binary64 little-endian bits)
      3 -> string (value_len = N; raw UTF-8, no NUL terminator)
```

`slot_count` is the descriptor's `structural_params.len()`. A blob whose
header `slot_count` disagrees is rejected with
`StructuralDecodeError::SlotCountMismatch`. There is no padding
anywhere — the format is tightly packed.

An empty descriptor produces the two-byte blob `00 00`.

```rust
pub const TAG_BOOL:   u8 = 0;
pub const TAG_I64:    u8 = 1;
pub const TAG_F64:    u8 = 2;
pub const TAG_STRING: u8 = 3;
```

### Tag/length invariants

Decoders must reject:

- `TAG_BOOL` with `value_len != 1`
- `TAG_I64` with `value_len != 8`
- `TAG_F64` with `value_len != 8`
- Any tag value not in `{0, 1, 2, 3}`
- A `TAG_STRING` payload that is not valid UTF-8

`TAG_STRING` accepts any `value_len`, including zero.

### Worked encoding examples

A `bool true` slot:

```text
01            tag = TAG_BOOL
01 00 00 00   value_len = 1
01            payload = true
```

An `i64 -3` slot:

```text
01            tag = TAG_I64
08 00 00 00   value_len = 8
fd ff ff ff ff ff ff ff   payload = (-3).to_le_bytes()
```

An `f64 0.25` slot:

```text
02            tag = TAG_F64
08 00 00 00   value_len = 8
00 00 00 00 00 00 d0 3f   payload = 0.25_f64.to_le_bytes()
```

A `string "kick.wav"` slot:

```text
03            tag = TAG_STRING
08 00 00 00   value_len = 8
6b 69 63 6b 2e 77 61 76   payload = "kick.wav" UTF-8 bytes
```

A complete blob for `[Bool(true), Int(-3), Float(0.25), String("kick.wav")]`:

```text
04 00                                slot_count = 4
01  01 00 00 00  01                  Bool(true)
01  08 00 00 00  fd ff ff ff ff ff ff ff   Int(-3)
02  08 00 00 00  00 00 00 00 00 00 d0 3f   Float(0.25)
03  08 00 00 00  6b 69 63 6b 2e 77 61 76   String("kick.wav")
```

### Float width

The descriptor's `ParameterKind::Float` declares `f32`, but the wire
format carries `f64`. The host widens on encode; the plugin SDK narrows
on decode. This is intentional: structural floats are construction-time
values, so the extra precision is free and lets the same encoder serve
hosts whose parameter editor stores doubles.

### File slots

`ParameterKind::File` has no compile-time default. When a host packs a
blob that omits a file slot, the slot is encoded as `TAG_STRING` with
`value_len = 0`. The plugin should treat an empty string as
“unspecified” and use whatever fallback is appropriate (typically the
zero-length sample buffer).

### `SongName`

`ParameterKind::SongName` is wire-encoded as `TAG_I64`. The reference
host emits `-1` when no song is selected.

---

## 4. `CableValue` and the cycle slot

The cable pool is the shared signal carrier between modules. Plugins
receive raw pointers into it on every `process` and `periodic_update`
call.

### `CableValue`

```rust
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CableValue(pub [f32; 16]);
```

- 64 bytes, alignment 4.
- No padding, no tag.
- Lanes are IEEE-754 `binary32`, little-endian on every supported
  target.
- `Default` and `ZERO` yield all-zero lanes.

### Lane semantics

The cable's declared `CableKind` (carried on the corresponding
`PortDescriptor` in the descriptor JSON) determines which lanes are
meaningful for a reader. Writers zero the unused prefix bytes when
constructing values via `CableValue::mono` / `CableValue::stereo`, but
this is not required by the contract — readers must not inspect lanes
outside their kind's prefix.

| `CableKind` | Meaningful lanes | Reader interpretation               |
| ----------- | ---------------- | ----------------------------------- |
| `Mono`      | `[0]`            | `value.0[0]` is the sample          |
| `Stereo`    | `[0, 1]`         | `(L, R) = (value.0[0], value.0[1])` |
| `Poly`      | `[0..16)`        | full 16-lane voice array            |

`Mono` may carry an `audio` or `trigger` sub-layout, and `Poly` may carry
`audio` / `trigger` / `transport` / `midi`. These layouts do not change
the byte shape of the lanes — they constrain how the sample values are
interpreted. The descriptor schema lists the layout tags.

### Scratch slots

A scratch slot is a single `CableValue`, 64 bytes, alignment 4.

`process` receives a contiguous run of scratch slots via
`(scratch_ptr: *mut CableValue, scratch_len: usize)`. The plugin reads
and writes by index. Both read and write target the same slot; there is
no ping-pong in scratch.

Producers in scratch are scheduled before their consumers by the
planner's topological order, so consumers observe the producer's
current-tick output — no one-sample delay. This is the “fused” property
generalised to a whole region; see ADR 0072.

### Cycle slots

A cycle slot is `[CableValue; 2]`, 128 bytes, alignment 4. The pair is
a ping-pong buffer keyed on a single `write_index` (`wi`) shared by the
whole tick.

`process` receives `(cycle_ptr: *mut [CableValue; 2], cycle_len: usize,
write_index: usize)` and indexes cycle slots by
`cable_idx - SCRATCH_CAPACITY`.

Default semantics (delayed-edge readers):

```text
producer writes slot[wi]
consumer reads  slot[1 - wi]
```

This gives every cycle edge a one-sample delay and makes execution
order irrelevant for the producer/consumer pair: the consumer always
reads what the producer wrote on the *previous* tick.

Fused-edge readers (input port with `FfiInputPort.fused = 1`):

```text
producer writes slot[wi]
consumer reads  slot[wi]
```

When a cycle slot is in a fused acyclic region, the planner has
guaranteed that the producer runs before the consumer within this tick.
The consumer reads the producer's current-tick output instead of the
previous-tick one. This is ADR 0072 phase 2.

`fused` is signalled per consumer in the input port (`FfiInputPort.fused`),
not per cable. A single producer with two readers — one fused, one
delayed — is permitted; the writer's behaviour is the same either way.

Producers always write `slot[wi]`. There is no “fused write” path.

### Plugin reconstruction

The reference Rust SDK reconstructs a typed `CablePool` from the raw
pointers:

```rust
CablePool::new(scratch_slice, cycle_slice, write_index)
```

External SDKs are free to bypass that abstraction and index the raw
slices directly. The byte layout is what is normative; the `CablePool`
type is host-side ergonomics.

---

## Load-time descriptor hash

Each plugin module exports a stateless C function

```c
uint64_t patches_plugin_descriptor_hash_<module_name>(void);
```

which returns a 64-bit FNV-1a digest of the module's descriptor. The
host computes the same digest from its own copy of the descriptor at
load time and refuses to register the plugin module on mismatch. This
detects descriptor drift between the source the host was compiled
against and the source the plugin was compiled against.

### Canonical byte encoding

The hash input is built deterministically from the descriptor:

1. `module_name` as `len: u32` then UTF-8 bytes.
2. Parameters in canonical `(name, index)` order:
   - `param_count: u32`
   - for each: `name` as `len: u32` then UTF-8 bytes; `index: u32`;
     `kind_tag: u8`; kind payload (see below).
3. Inputs in **declared** order (descriptor slot order is the
   `Module::process` slice index — reordering here would break the
   module contract, so port order is part of the shape):
   - `input_count: u32`
   - for each input: `name: u32 + UTF-8`; `index: u32`;
     `kind_tag: u8`; `mono_layout_tag: u8`; `poly_layout_tag: u8`.
4. Outputs in declared order: identical encoding to inputs.

`kind_tag` values for parameters:

| Kind         | Tag |
| ------------ | --: |
| `Float`      |   0 |
| `Int`        |   1 |
| `Bool`       |   2 |
| `Enum`       |   3 |
| `File`       |   4 |
| `SongName`   |   5 |

Kind payloads: `Enum` writes `variants.len(): u32` followed by each
variant string (`len: u32` + UTF-8). All other kinds write no payload
(range and default are clamping behaviour, not wire shape, and may be
tuned without forcing a hash bump).

`kind_tag` values for cables (used for both port kind and the
descriptor's structural `CableKind` references):

| `CableKind` | Tag |
| ----------- | --: |
| `Mono`      |   0 |
| `Poly`      |   1 |
| `Stereo`    |   2 |

`mono_layout_tag`:

| `MonoLayout` | Tag |
| ------------ | --: |
| `Audio`      |   0 |
| `Trigger`    |   1 |

`poly_layout_tag`:

| `PolyLayout` | Tag |
| ------------ | --: |
| `Audio`      |   0 |
| `Trigger`    |   1 |
| `Transport`  |   2 |
| `Midi`       |   3 |

### Digest

FNV-1a 64-bit over the byte sequence above:

```text
offset = 0xcbf29ce484222325
prime  = 0x00000100000001b3

state  = offset
for each byte b:
    state ^= b
    state  = state * prime   (wrapping)
return state
```

This is not a cryptographic hash; the threat model is accidental drift
between source trees. If that bar ever rises, the digest function can
be swapped without changing the canonical byte encoding feeding it —
but that swap **is** an ABI bump.

---

## Cross-references

- [Descriptor schema](./descriptor-schema.md) — the JSON shape that
  names every parameter and port referenced from the wire formats above.
- ADR 0045 — packed parameter / port frame design.
- ADR 0060 — structural parameter slot design.
- ADR 0068 — single 16-lane `CableValue` slot for all kinds.
- ADR 0072 — fused acyclic regions and the `FfiInputPort.fused` flag.
- The `FfiBytes` allocation contract (plugin allocates, plugin frees via
  `FfiPluginVTable::free_bytes`) is documented with the vtable; the
  bytes are opaque on the wire.
