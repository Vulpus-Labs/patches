# Descriptor JSON schema

This page is the normative reference for the JSON blobs that cross the
Patches FFI plugin ABI. External SDKs — including SDKs in languages
other than Rust — should implement to this spec.

The canonical implementation lives in `patches-ffi-common/src/json/`
(deserializer in `de.rs`, serializer in `ser.rs`, template ser/de in
`template.rs`). The implementation is hand-rolled with no
`serde`/`serde_json` dependency: the schema is the host's, not
serde's.

See also: [Wire formats](wire-formats.md) for the binary packing of
`ParamFrame`, `PortFrame`, the structural blob, and `CableValue` (out
of scope here).

## The two JSON crossings

JSON crosses the FFI boundary in exactly two places, both on the
control thread. Audio-thread traffic is binary and is documented in
[Wire formats](wire-formats.md).

| When                  | Direction       | Payload                       | Purpose                                                  |
| --------------------- | --------------- | ----------------------------- | -------------------------------------------------------- |
| Plugin load           | plugin → host   | `ModuleDescriptorTemplate`    | Static, shape-axis-parameterised description of a module |
| Instance construction | host → plugin   | `ModuleDescriptor`            | Per-instance descriptor with axis counts resolved        |

At load time the host calls the plugin's `module_template()` vtable
entry, which returns a serialised `ModuleDescriptorTemplate`. The host
deserialises it once and uses it to build per-instance descriptors
without re-entering the plugin.

At instance construction the host serialises the resulting
`ModuleDescriptor` and passes the bytes to the plugin's `prepare()`
vtable entry as `descriptor_json` / `descriptor_json_len`.

The two blobs are related but not interchangeable: the template
carries unresolved port and parameter declarations parameterised by
named count axes, while the instance descriptor has those axes
resolved to concrete `index` values.

## JSON dialect

The parser accepts standard JSON with no extensions. Specifically:

- Object keys are case-sensitive and matched as exact strings.
- Numbers are decoded through IEEE-754 double; integer fields silently
  truncate (`as i64`) and `usize` fields silently widen.
- Strings use the standard JSON escape set (`\"`, `\\`, `\/`, `\n`,
  `\r`, `\t`, `\uXXXX`); any other escape character is passed through
  as itself.
- Unrecognised object keys at the top level or within nested objects
  are silently ignored. This is the additive-extension hook: future
  fields land here without breaking older parsers.
- Missing optional fields fall back to documented defaults. Missing
  required fields are a deserialisation error.

Whitespace between tokens is not significant. The canonical
serializer emits compact JSON with no whitespace.

## `ModuleDescriptor`

The per-instance descriptor. Shape: a single object.

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

| Field               | JSON type   | Required | Default | Description                                                                |
| ------------------- | ----------- | -------- | ------- | -------------------------------------------------------------------------- |
| `module_name`       | string      | yes      | —       | Module type name (e.g. `"Osc"`, `"Gain"`). Must match registry entry.      |
| `shape`             | object      | yes      | —       | See [`ModuleShape`](#moduleshape).                                         |
| `inputs`            | array       | no       | `[]`    | Input ports in slice order. See [`PortDescriptor`](#portdescriptor).       |
| `outputs`           | array       | no       | `[]`    | Output ports in slice order.                                               |
| `realtime_params`   | array       | no       | `[]`    | Audio-thread parameters. See [`ParameterDescriptor`](#parameterdescriptor).|
| `structural_params` | array       | no       | `[]`    | Control-thread-only parameters (ADR 0060).                                 |

The declared order of `inputs` and `outputs` *is* the slice index
passed to the module's `process()` callback. Reordering this list
changes the contract with the module implementation and bumps the
descriptor hash.

### `ModuleShape`

```json
{ "channels": 1 }
```

| Field      | JSON type | Required | Default | Description                                                              |
| ---------- | --------- | -------- | ------- | ------------------------------------------------------------------------ |
| `channels` | number    | no       | `0`     | Count for the `channels` shape axis. Must be ≥ 1 to pass host validation. |

`shape` is a self-contained object so future shape axes can be added
without revising the top-level schema. Today only `channels` is
surfaced. The deserializer accepts `0` as a default but the host's
`ModuleShape::validate` rejects it; plugins should always emit a
positive value.

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

| Field         | JSON type | Required | Default   | Description                                                                                 |
| ------------- | --------- | -------- | --------- | ------------------------------------------------------------------------------------------- |
| `name`        | string    | yes      | —         | Port name. Must be a `&'static str` on the Rust side; the host leaks the decoded string.    |
| `index`       | number    | no       | `0`       | User-visible index within a multi-port group (`in/2` → `index: 2`).                         |
| `kind`        | string    | no       | `"mono"`  | Cable arity. One of `"mono"`, `"poly"`, `"stereo"`. Unknown values fall back to `"mono"`.   |
| `mono_layout` | string    | no       | `"audio"` | Mono semantic layout. One of `"audio"`, `"trigger"`. Unknown values fall back to `"audio"`. |
| `poly_layout` | string    | no       | `"audio"` | Poly semantic layout. One of `"audio"`, `"trigger"`, `"transport"`, `"midi"`. Unknown values fall back to `"audio"`. |

Layouts must match exactly across a connection (ADR 0033, ADR 0047).
The graph connection validator inspects only the layout appropriate to
the port's arity: `mono` ports check `mono_layout` and ignore
`poly_layout`; `poly` ports check `poly_layout` and ignore
`mono_layout`. `stereo` ports ignore both.

### `ParameterDescriptor`

```json
{
  "name": "gain",
  "index": 0,
  "parameter_type": { "type": "float", "min": 0.0, "max": 4.0, "default": 1.0 }
}
```

| Field            | JSON type | Required | Default | Description                                                          |
| ---------------- | --------- | -------- | ------- | -------------------------------------------------------------------- |
| `name`           | string    | yes      | —       | Parameter name.                                                      |
| `index`          | number    | no       | `0`     | Index within a multi-param group.                                    |
| `parameter_type` | object    | yes      | —       | A [`ParameterKind`](#parameterkind) tagged-union object.             |

Note: the per-instance schema uses `parameter_type` as the kind-payload
key. The template schema uses `kind` for the same payload. Plugins
must use the correct key for the blob they are emitting.

### `ParameterKind`

A tagged union discriminated by the `type` string field. The
deserializer rejects any other `type` value as an error.

#### `Float`

```json
{ "type": "float", "min": 0.0, "max": 1.0, "default": 0.0 }
```

| Field     | JSON type | Required | Default | Description                                            |
| --------- | --------- | -------- | ------- | ------------------------------------------------------ |
| `type`    | string    | yes      | —       | Literal `"float"`.                                     |
| `min`     | number    | no       | `0.0`   | Inclusive lower bound. Decoded as `f32`.               |
| `max`     | number    | no       | `1.0`   | Inclusive upper bound. Decoded as `f32`.               |
| `default` | number    | no       | `0.0`   | Initial value. Decoded as `f32`.                       |

The serializer emits `min`, `max`, and `default` as JSON numbers with a
decimal point or exponent (e.g. `1.0`, not `1`). Non-finite values are
emitted as `null` (NaN) or `1e38`/`-1e38` (infinity); plugins should
not rely on round-trip of those edge cases.

Valid in `realtime_params` and `structural_params`.

#### `Int`

```json
{ "type": "int", "min": 0, "max": 100, "default": 0 }
```

| Field     | JSON type | Required | Default | Description                                       |
| --------- | --------- | -------- | ------- | ------------------------------------------------- |
| `type`    | string    | yes      | —       | Literal `"int"`.                                  |
| `min`     | number    | no       | `0`     | Inclusive lower bound. Truncated to `i64`.        |
| `max`     | number    | no       | `100`   | Inclusive upper bound. Truncated to `i64`.        |
| `default` | number    | no       | `0`     | Initial value. Truncated to `i64`.                |

Valid in `realtime_params` and `structural_params`.

#### `Bool`

```json
{ "type": "bool", "default": false }
```

| Field     | JSON type | Required | Default | Description                       |
| --------- | --------- | -------- | ------- | --------------------------------- |
| `type`    | string    | yes      | —       | Literal `"bool"`.                 |
| `default` | boolean   | no       | `false` | Initial value.                    |

Valid in `realtime_params` and `structural_params`.

#### `Enum`

```json
{ "type": "enum", "variants": ["sine", "saw", "square"], "default": "sine" }
```

| Field      | JSON type    | Required | Default | Description                                                          |
| ---------- | ------------ | -------- | ------- | -------------------------------------------------------------------- |
| `type`     | string       | yes      | —       | Literal `"enum"`.                                                    |
| `variants` | string array | yes      | —       | Variant identifiers. Order is significant: the index maps to wire value. |
| `default`  | string       | no       | `""`    | Must equal one of `variants`. Empty default falls back to variant 0 at runtime. |

Variant order is part of the descriptor hash. Adding a variant in the
middle of the list, or reordering, changes the hash and forces a
load-time refusal.

Valid in `realtime_params` only.

#### `File` (structural-only)

```json
{ "type": "file", "extensions": ["wav", "aiff"] }
```

| Field        | JSON type    | Required | Default | Description                                                      |
| ------------ | ------------ | -------- | ------- | ---------------------------------------------------------------- |
| `type`       | string       | yes      | —       | Literal `"file"`.                                                |
| `extensions` | string array | no       | `[]`    | Accepted file extensions (lower-case, no leading dot).           |

The host interprets the DSL `file("path")` form, resolves it against
the patch file's directory, validates the extension, and threads the
absolute path through the structural blob to `prepare()`.

**Valid in `structural_params` only.** Emitting `File` in
`realtime_params` is undefined behaviour: the host's param-layout
computation panics with "ParameterKind::File only valid in
structural_params" (`patches-core/src/param_layout/mod.rs`).

#### `SongName` (structural-only by convention)

```json
{ "type": "song_name" }
```

| Field  | JSON type | Required | Default | Description              |
| ------ | --------- | -------- | ------- | ------------------------ |
| `type` | string    | yes      | —       | Literal `"song_name"`.   |

No payload fields. The DSL writes `song: "my_song"` and the
interpreter resolves it to an integer index into the alphabetically
sorted song bank, then passes it as `ParameterValue::Int` (`-1` if
unresolved).

`SongName` is registered as a *structural* string-resolution kind
alongside `File`. The descriptor builder reserves
`song_name_param(...)` for the **realtime** path, where the resolved
index is packed as an `Int` scalar (tag `ScalarTag::Int`). External
SDKs that emit `SongName` in `realtime_params` will work; external
SDKs that emit it in `structural_params` are also accepted by the
schema but the host does not currently have a code path that consumes
it from there.

### Which kinds are valid where

| Kind       | `realtime_params` | `structural_params` |
| ---------- | ----------------- | ------------------- |
| `Float`    | yes               | yes                 |
| `Int`      | yes               | yes                 |
| `Bool`     | yes               | yes                 |
| `Enum`     | yes               | not used today      |
| `File`     | **rejected**      | yes                 |
| `SongName` | yes               | accepted but unused |

`Float`, `Int`, and `Bool` are used in both lists by built-in modules.
`Enum` and `SongName` are emitted into `realtime_params` by the
descriptor builder helpers; the deserializer will accept them in
`structural_params` but no built-in module ships that combination.

`File` is the only kind that is *rejected* outside its list: the
realtime param-layout path panics on it and the host's
`ParameterKind::default_value()` panics on it. Plugins must keep
`File` parameters in `structural_params`.

## `ModuleDescriptorTemplate`

The static, axis-parameterised description returned by
`module_template()`. Shape: a single object.

```json
{
  "name": "Gain",
  "axes": ["channels"],
  "global_inputs":  [ /* PortTemplate */ ],
  "per_axis_inputs":  [ /* axis-tagged PortTemplate */ ],
  "global_outputs": [ /* PortTemplate */ ],
  "per_axis_outputs": [ /* axis-tagged PortTemplate */ ],
  "realtime_params":   [ /* ParameterTemplate */ ],
  "structural_params": [ /* ParameterTemplate */ ],
  "per_axis_realtime_params":   [ /* axis-tagged ParameterTemplate */ ],
  "per_axis_structural_params": [ /* axis-tagged ParameterTemplate */ ]
}
```

| Field                          | JSON type    | Required | Default | Description                                                  |
| ------------------------------ | ------------ | -------- | ------- | ------------------------------------------------------------ |
| `name`                         | string       | yes      | —       | Module type name.                                            |
| `axes`                         | string array | no       | `[]`    | Names of count axes referenced by per-axis entries. Today the only surfaced axis is `"channels"`. |
| `global_inputs`                | array        | no       | `[]`    | Ports that appear once regardless of axis count.             |
| `per_axis_inputs`              | array        | no       | `[]`    | Ports fanned out per axis. Each entry is `{"axis": …, "port": PortTemplate}`. |
| `global_outputs`               | array        | no       | `[]`    | As `global_inputs`.                                          |
| `per_axis_outputs`             | array        | no       | `[]`    | As `per_axis_inputs`.                                        |
| `realtime_params`              | array        | no       | `[]`    | Global realtime parameters. See [`ParameterTemplate`](#parametertemplate). |
| `structural_params`            | array        | no       | `[]`    | Global structural parameters.                                |
| `per_axis_realtime_params`     | array        | no       | `[]`    | Realtime parameters fanned out per axis. Each entry is `{"axis": …, "param": ParameterTemplate}`. |
| `per_axis_structural_params`   | array        | no       | `[]`    | Structural parameters fanned out per axis.                   |

The host builds a per-instance `ModuleDescriptor` by walking these
collections in order:

1. Each entry in `global_inputs` becomes one `PortDescriptor` with
   `index: 0`.
2. Each entry in `per_axis_inputs` is expanded to `N` `PortDescriptor`
   values with indices `0..N`, where `N` is the count supplied for
   the entry's axis.
3. Outputs and parameters follow the same rule.

The resulting `inputs` / `outputs` / `realtime_params` /
`structural_params` arrays carry the global entries first, followed by
the per-axis fan-out — this order is part of the descriptor's wire
shape.

### `PortTemplate`

```json
{ "name": "in", "kind": "mono", "mono_layout": "audio", "poly_layout": "audio" }
```

| Field         | JSON type | Required | Default   | Description                                          |
| ------------- | --------- | -------- | --------- | ---------------------------------------------------- |
| `name`        | string    | yes      | —         | Port name.                                           |
| `kind`        | string    | no       | `"mono"`  | As [`PortDescriptor.kind`](#portdescriptor).         |
| `mono_layout` | string    | no       | `"audio"` | As [`PortDescriptor.mono_layout`](#portdescriptor).  |
| `poly_layout` | string    | no       | `"audio"` | As [`PortDescriptor.poly_layout`](#portdescriptor).  |

`PortTemplate` has no `index` field — it is filled in at build time
from the port's position within its axis fan-out.

### Axis-tagged port entries

Entries in `per_axis_inputs` and `per_axis_outputs` wrap a
`PortTemplate` with the axis it fans out over:

```json
{ "axis": "channels", "port": { "name": "in", "kind": "mono", "mono_layout": "audio", "poly_layout": "audio" } }
```

| Field  | JSON type           | Required | Default | Description                              |
| ------ | ------------------- | -------- | ------- | ---------------------------------------- |
| `axis` | string              | yes      | —       | Must match an entry in the top-level `axes` list. |
| `port` | object              | yes      | —       | A [`PortTemplate`](#porttemplate).       |

### `ParameterTemplate`

```json
{ "name": "gain", "kind": { "type": "float", "min": 0.0, "max": 4.0, "default": 1.0 } }
```

| Field  | JSON type | Required | Default | Description                                         |
| ------ | --------- | -------- | ------- | --------------------------------------------------- |
| `name` | string    | yes      | —       | Parameter name.                                     |
| `kind` | object    | yes      | —       | A [`ParameterKind`](#parameterkind) tagged-union object. |

Note the key difference from `ParameterDescriptor`: the template uses
`kind`, the per-instance descriptor uses `parameter_type`. The
tagged-union payload is identical.

### Axis-tagged parameter entries

Entries in `per_axis_realtime_params` and `per_axis_structural_params`
wrap a `ParameterTemplate`:

```json
{ "axis": "channels", "param": { "name": "gain", "kind": { "type": "float", "min": 0.0, "max": 1.0, "default": 1.0 } } }
```

| Field   | JSON type | Required | Default | Description                                |
| ------- | --------- | -------- | ------- | ------------------------------------------ |
| `axis`  | string    | yes      | —       | Must match an entry in the top-level `axes` list. |
| `param` | object    | yes      | —       | A [`ParameterTemplate`](#parametertemplate). |

## Stability guarantees

The instance descriptor feeds an FNV-1a-64 `descriptor_hash` (see
`patches-core/src/param_layout/hash.rs`) which the host checks against
the plugin's reported hash at instance construction. Mismatch is a
load-time refusal.

Fields that **are** part of the hash (changing them forces a hash
bump):

- `module_name`.
- Each realtime parameter's `name`, `index`, and kind tag (`Float`,
  `Int`, `Bool`, `Enum`, `SongName` — `File` is rejected and
  unreachable on this path).
- For `Enum` kinds, the full ordered list of variant strings.
- Each port's `name`, `index`, `kind`, `mono_layout`, and
  `poly_layout`, in declared order across `inputs` then `outputs`.
- The number of realtime parameters and the number of ports in each
  list.

Realtime parameters are canonicalised by `(name, index)` before being
fed into the hash; declaration order of realtime params does not
affect the hash. Ports are *not* canonicalised — their declared order
is the slice index passed to `Module::process()`, so reordering them
breaks the contract with the module implementation.

Fields that **are not** part of the hash (additive-friendly; tune
freely):

- `min`, `max`, and `default` for `Float` and `Int` kinds.
- `default` for `Bool` kinds.
- `default` for `Enum` kinds (the variant *list* is hashed; the chosen
  default within that list is not).
- `extensions` for `File` kinds (in any case, `File` does not reach
  the realtime hash path).
- `structural_params` in its entirety. The hash covers realtime
  parameters and ports only.
- `shape.channels`. Per-instance shape values are not folded into the
  hash; two instances of the same module with different channel counts
  hash differently only via the resulting port and parameter
  fan-out, which the hash *does* see.
- Unrecognised JSON keys. Parsers ignore them.

The rationale for excluding ranges and defaults from the hash is in
`hash.rs` lines 99–107: range and default are clamping behaviour, not
wire layout. Tuning a knob's range should not refuse a previously
working instance.

### Adding fields

The schema is forward-compatible at the JSON layer: parsers ignore
unrecognised keys, so a future spec revision can add fields without
breaking old plugins. Whether a *new* field is descriptor-hash-relevant
is a separate decision made when the field is added to the canonical
encoding in `hash.rs`.

### Removing or renaming fields

Removing or renaming any field documented above is an ABI break and
must be paired with an `abi_version` bump in the plugin vtable
(`FfiPluginVTable::abi_version`). The host refuses plugins whose
`abi_version` does not match its expected value.

## Worked example

A minimal two-port, one-parameter module with one channel axis. The
module type is `Gain`: one mono audio input `in`, one mono audio
output `out`, and one `Float` realtime parameter `gain`.

### Template JSON

What the plugin returns from `module_template()`:

```json
{
  "name": "Gain",
  "axes": [],
  "global_inputs": [
    { "name": "in", "kind": "mono", "mono_layout": "audio", "poly_layout": "audio" }
  ],
  "per_axis_inputs": [],
  "global_outputs": [
    { "name": "out", "kind": "mono", "mono_layout": "audio", "poly_layout": "audio" }
  ],
  "per_axis_outputs": [],
  "realtime_params": [
    { "name": "gain", "kind": { "type": "float", "min": 0.0, "max": 4.0, "default": 1.0 } }
  ],
  "structural_params": [],
  "per_axis_realtime_params": [],
  "per_axis_structural_params": []
}
```

No axes are declared (this module's shape is fixed); both ports and
the parameter are global. If the module were per-channel, the input
would move into `per_axis_inputs` with `{"axis": "channels", "port":
…}` and `"channels"` would appear in `axes`.

### Instance JSON

What the host passes to `prepare()` after building the template with
`channels = 1`:

```json
{
  "module_name": "Gain",
  "shape": { "channels": 1 },
  "inputs": [
    { "name": "in", "index": 0, "kind": "mono", "mono_layout": "audio", "poly_layout": "audio" }
  ],
  "outputs": [
    { "name": "out", "index": 0, "kind": "mono", "mono_layout": "audio", "poly_layout": "audio" }
  ],
  "realtime_params": [
    {
      "name": "gain",
      "index": 0,
      "parameter_type": { "type": "float", "min": 0.0, "max": 4.0, "default": 1.0 }
    }
  ],
  "structural_params": []
}
```

### Side-by-side differences

| Aspect                       | Template                                | Instance                                       |
| ---------------------------- | --------------------------------------- | ---------------------------------------------- |
| Top-level name key           | `name`                                  | `module_name`                                  |
| Shape carrier                | `axes` (array of axis names)            | `shape` (object of axis counts)                |
| Port/param `index` field     | Absent (filled at build time)           | Required (default `0`)                        |
| Per-axis fan-out             | Separate `per_axis_*` arrays            | Already expanded into `inputs`/`outputs`/etc. |
| Parameter kind payload key   | `kind`                                  | `parameter_type`                               |
| Kind payload structure       | Identical                               | Identical                                      |

## Schema versioning

This schema is versioned alongside `FfiPluginVTable::abi_version`. The
JSON dialect, key set, and tagged-union discriminator values described
on this page constitute the contract for the current `abi_version`.

Additive changes (new optional fields, new tagged-union variants
guarded by `type`) can land without bumping `abi_version`, provided
existing parsers continue to ignore them safely.

Breaking changes (rename, removal, type change, default-behaviour
change) require an `abi_version` bump and a matching update to this
page.
