# Native plugin ABI

This is the reference for the C ABI between a Patches host and an
external native-module plugin. The ABI lets you distribute compiled
module bundles as `.dylib` / `.so` / `.dll` files separately from
the main Patches build — including bundles written in languages
other than Rust.

The Rust SDK (`patches-sdk`, covered in
[Writing a native plugin](extending-native-plugin.md)) hides this
ABI behind ergonomic types. You only need this chapter if you're
writing a plugin without the SDK — for example, an SDK for another
language.

## Stability

**The ABI is not yet stable.** Each shipped Patches release pins a
specific `ABI_VERSION`; plugins built against a different version
are refused at load time. The version is currently **12**
(`patches-ffi-common/src/types.rs:ABI_VERSION`).

The shape is unlikely to change drastically, but the byte layouts
of `ParamFrame`, `PortFrame`, and the structural blob have evolved
between versions and may evolve again. Treat the current
documentation as a snapshot; track the source.

Canonical sources of truth:

- `patches-ffi-common/src/` — type definitions, JSON
  (de)serializers, wire format encoders.
- `patches-ffi/src/loader.rs` — the host's side of the loading
  protocol.
- `patches-sdk/` — the Rust SDK; the simplest reference
  implementation of "what a plugin does".

## ABI surface in one diagram

```text
   ┌─────── plugin (cdylib) ──────────────┐
   │                                       │
   │   patches_plugin_init()               │  ← exported symbol
   │       → FfiPluginManifest             │
   │           ├─ abi_version: u32         │
   │           ├─ count: usize             │
   │           └─ vtables: *const          │
   │              FfiPluginVTable[count]   │
   │                                       │
   │   Each FfiPluginVTable:               │
   │       module_template()  → JSON       │  ← load-time
   │       prepare(...)       → handle     │  ← per instance
   │       update_validated_parameters()   │  ← audio thread
   │       set_ports()                     │  ← audio thread
   │       process()                       │  ← audio thread
   │       periodic_update()               │  ← control thread
   │       drop()                          │  ← per instance
   │       free_bytes()                    │  ← helper
   │                                       │
   └───────────────────────────────────────┘
```

## Loading protocol

1. The host scans a bundle directory and `dlopen`s each `.dylib` /
   `.so` / `.dll`.
2. It resolves the `patches_plugin_init` symbol and calls it. The
   plugin returns an `FfiPluginManifest`:

   ```rust
   #[repr(C)]
   pub struct FfiPluginManifest {
       pub abi_version: u32,
       pub count: usize,
       pub vtables: *const FfiPluginVTable,
   }
   ```

3. The host checks `abi_version`. Mismatch → bundle refused, error
   logged, no modules registered.
4. For each of the `count` vtables, the host calls
   `module_template()`, receives a JSON `ModuleDescriptorTemplate`
   blob, deserialises it once, and registers the module type.
5. Per-instance work happens later, when a patch instantiates a
   module of that type.

## The two JSON crossings

JSON crosses the FFI boundary in exactly two places, both on the
control thread. Audio-thread traffic is binary.

| When                  | Direction     | Payload                       | Purpose                                                  |
| --------------------- | ------------- | ----------------------------- | -------------------------------------------------------- |
| Plugin load           | plugin → host | `ModuleDescriptorTemplate`    | Static, shape-axis-parameterised description of a module |
| Instance construction | host → plugin | `ModuleDescriptor`            | Per-instance descriptor with axis counts resolved        |

The two blobs share field names but encode different lifecycle
stages:

- A **template** carries unresolved port and parameter declarations
  parameterised by named count axes (e.g. "one input port per
  channel").
- An **instance descriptor** has those axes resolved to concrete
  index values for a specific instantiation (e.g. "channels = 3, so
  three input ports").

The deserialiser is hand-rolled (no `serde_json` dependency); the
schema is the host's, not serde's.

### JSON dialect

Standard JSON, no extensions:

- Object keys case-sensitive, exact-string matched.
- Numbers decoded through IEEE-754 double; integer fields silently
  truncate (`as i64`); `usize` fields silently widen.
- String escapes standard (`\"`, `\\`, `\/`, `\n`, `\r`, `\t`,
  `\uXXXX`); other escapes pass through as the literal character.
- **Unrecognised keys are silently ignored.** This is the
  additive-extension hook: new fields land here without breaking
  older parsers.
- Missing optional fields fall back to documented defaults; missing
  required fields are an error.

The canonical serialiser emits compact JSON (no whitespace).

## `ModuleDescriptor` (instance)

```json
{
  "module_name": "Gain",
  "shape": { "channels": 1 },
  "inputs":  [ /* PortDescriptor */ ],
  "outputs": [ /* PortDescriptor */ ],
  "realtime_params":   [ /* ParameterDescriptor */ ],
  "structural_params": [ /* ParameterDescriptor */ ]
}
```

| Field               | JSON type | Required | Default | Description                                                                |
| ------------------- | --------- | -------- | ------- | -------------------------------------------------------------------------- |
| `module_name`       | string    | yes      | —       | Module type name. Must match registry entry.                               |
| `shape`             | object    | yes      | —       | See `ModuleShape` below.                                                   |
| `inputs`            | array     | no       | `[]`    | Input ports in slice order.                                                |
| `outputs`           | array     | no       | `[]`    | Output ports in slice order.                                               |
| `realtime_params`   | array     | no       | `[]`    | Audio-thread parameters.                                                   |
| `structural_params` | array     | no       | `[]`    | Control-thread-only parameters.                                 |

The declared order of `inputs` and `outputs` *is* the slice index
passed to the module's `process()` callback. Reordering changes
the contract.

### `ModuleShape`

```json
{ "channels": 1 }
```

`shape` is a self-contained object so future shape axes can be
added without revising the top-level schema. Today only `channels`
is exposed. Must be ≥ 1.

### `PortDescriptor`

```json
{
  "name": "in",
  "index": 0,
  "kind": "mono",
  "mono_layout": "audio",
  "poly_layout": "audio"
}
```

| Field         | JSON type | Required | Default   | Description                                                                              |
| ------------- | --------- | -------- | --------- | ---------------------------------------------------------------------------------------- |
| `name`        | string    | yes      | —         | Port name.                                                                               |
| `index`       | number    | no       | `0`       | Index within a multi-port group.                                                         |
| `kind`        | string    | no       | `"mono"`  | One of `"mono"`, `"poly"`, `"stereo"`. Unknown → `"mono"`.                               |
| `mono_layout` | string    | no       | `"audio"` | One of `"audio"`, `"trigger"`.                                                           |
| `poly_layout` | string    | no       | `"audio"` | One of `"audio"`, `"trigger"`, `"transport"`, `"midi"`.                                  |

Layouts must match across a connection. Only
the layout matching the port's `kind` is checked.

### `ParameterDescriptor`

```json
{
  "name": "gain",
  "index": 0,
  "parameter_type": { "type": "float", "min": 0.0, "max": 4.0, "default": 1.0 }
}
```

`parameter_type` is a tagged union (the template form uses `kind`
for the same payload — note the key difference between instance
and template JSON).

### `ParameterKind` variants

| Tag      | Wire form                                                                  | Valid in              |
| -------- | -------------------------------------------------------------------------- | --------------------- |
| `float`  | `{ "type": "float", "min": …, "max": …, "default": … }`                    | realtime + structural |
| `int`    | `{ "type": "int", "min": …, "max": …, "default": … }`                      | realtime + structural |
| `bool`   | `{ "type": "bool", "default": … }`                                         | realtime + structural |
| `enum`   | `{ "type": "enum", "variants": [...], "default": "..." }`                  | realtime only         |
| `file`   | `{ "type": "file", "extensions": [...] }`                                  | structural only       |
| `song`   | `{ "type": "song" }` (or `{ "type": "song_name" }`)                        | structural only       |

`enum` variant *order* is part of the descriptor hash — adding or
reordering variants forces a load-time refusal. `file` paths are
host-resolved before being threaded through the structural blob.

For exhaustive field tables for each kind, see the canonical
implementations in `patches-ffi-common/src/json/`.

## `ModuleDescriptorTemplate` (load-time)

Shape parallels `ModuleDescriptor`, but ports and parameters are
tagged with optional axis-expansion metadata. The host expands them
during instance construction:

```json
{
  "name": "Gain",
  "axes": [ { "name": "channels" } ],
  "global_inputs":  [ /* PortTemplate */ ],
  "per_axis_inputs": [],
  "global_outputs": [ /* PortTemplate */ ],
  "per_axis_outputs": [],
  "realtime_params":   [ /* ParameterTemplate */ ],
  "structural_params": [],
  "per_axis_realtime_params":   [],
  "per_axis_structural_params": []
}
```

The split between **global** (one per module) and **per_axis** (one
per channel, expanded per shape-axis value at instance time) is the
template's reason for existing.

## Binary wire formats (audio thread)

Three packed binary frames cross the FFI boundary at audio rate:

| Frame              | When                              | Layout reference                                                                       |
| ------------------ | --------------------------------- | -------------------------------------------------------------------------------------- |
| `ParamFrame`       | `update_validated_parameters()`   | `patches-ffi-common/src/sdk.rs` (encoder), `patches-ffi-common/src/abi.rs` (ABI shape) |
| `PortFrame`        | `set_ports()`                     | `patches-ffi-common/src/port_frame.rs`                                                 |
| Structural blob    | `prepare()`                       | `patches-ffi-common/src/structural_frame.rs`                                           |

The cable-value pool itself is a split scratch / cycle region (ADR
0072, ABI v12 — see `FfiPluginVTable::process` doc comment for the
current layout). Plugins reconstruct a `CablePool` from the two
slices.

The byte-exact tables and worked encoding examples are too long to
duplicate here without staying in sync with the reference encoder.
Refer to the source files listed above; they are the contract.

## Descriptor hash

The host computes a stable hash over each instance descriptor
(module name, shape, ports, parameters in canonical order) and
gates load-time compatibility on it. A plugin can override the
hash with `export_plugin_with_hash_override!` if it knows a
descriptor change is wire-compatible; otherwise the default
canonical-byte digest is used.

Canonical encoding lives in `patches-ffi-common/src/sdk.rs` (search
for `descriptor_hash`). External SDKs must reproduce this byte
sequence exactly.

## Audio-thread rules

The same real-time constraints that apply to in-tree modules apply
across the FFI:

- No allocation on the audio thread (in `process`,
  `update_validated_parameters`, `set_ports`).
- No blocking — no mutexes, no I/O, no syscalls.
- No panics in plugins built with `panic = "abort"`. The Patches
  engine requires `panic = "unwind"`; FFI plugin crates must respect
  this so the host's tick-boundary catch_unwind can halt the engine
  cleanly instead of taking down the process.

## Implementing in another language

The minimum surface for an SDK in any C-ABI-compatible language:

1. Export `patches_plugin_init` returning an `FfiPluginManifest`.
2. Provide one `FfiPluginVTable` per module type with the eight
   function pointers wired up.
3. Implement `module_template` to return JSON matching the
   `ModuleDescriptorTemplate` schema above.
4. In `prepare`, parse the `ModuleDescriptor` JSON and store
   anything you need on the per-instance handle.
5. Implement `update_validated_parameters` and `set_ports` against
   the binary frame formats.
6. Implement `process` against the split scratch / cycle cable
   pool layout.
7. Compute the descriptor hash compatibly (or override).

The Rust SDK does all of this through the `export_plugin!` macro;
look at `patches-sdk/src/lib.rs` and a working plugin (e.g.
`test-plugins/gain/`) for an end-to-end reference.
