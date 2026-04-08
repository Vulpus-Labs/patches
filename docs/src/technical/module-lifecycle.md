# Module lifecycle & identity

## InstanceId

Every module instance is assigned an `InstanceId` at construction time:

```rust
pub struct InstanceId(u64);  // globally unique within a process run

impl InstanceId {
    pub fn next() -> Self { /* atomic u64 counter */ }
    pub fn as_u64(self) -> u64 { self.0 }
}
```

`InstanceId` is immutable for the lifetime of the instance. It is stored in the
module struct and returned by `Module::instance_id()`. The planner uses it to
match surviving modules from the old plan when building a new one.

## ModuleShape and structural identity

`ModuleShape` carries the arity of a module — the properties determined by
the `(channels: N)` argument in the DSL and by sequence/array lengths:

```rust
pub struct ModuleShape {
    pub channels: usize,   // e.g. number of mixer inputs
    pub length: usize,     // pre-allocated step count for sequencer-style modules
}
```

If a module's shape changes between builds (e.g. the number of mixer channels
changes), the planner considers it a different module and creates a fresh instance.
The old instance is tombstoned regardless of matching `InstanceId`.

Shape-independent parameters — those that don't change the port count or buffer
layout — are delivered as hot parameter updates to the surviving instance via
`update_validated_parameters`, not as a replacement.

## Construction protocol

Module construction is a two-phase process:

### Phase 1 — `describe(shape)`

A static method (called on the type, not an instance) that returns the
`ModuleDescriptor` for a given shape. This is called by the registry to inspect
port counts and parameter declarations without allocating an instance.

### Phase 2 — `build(env, shape, params, instance_id)`

The default `build` implementation:

1. Calls `describe(shape)` to obtain the descriptor.
2. Calls `prepare(env, descriptor, instance_id)` — allocates the instance with
   all fields at defaults. Infallible.
3. Fills any parameters the caller did not supply with the descriptor's declared
   defaults.
4. Calls `update_parameters(params)` — validates then applies.

Modules should not override `build` unless they need custom construction logic
beyond what `prepare` + `update_validated_parameters` can express.

## Parameter pipeline

Parameter changes flow through four stages:

```
DSL string map
     │
     ▼
patches-interpreter: ParameterMap (typed ParameterValue per key/index)
     │
     ▼
Module::update_parameters     ← validates against descriptor
     │ (on success)
     ▼
Module::update_validated_parameters   ← updates module fields
```

`update_parameters` is called both at construction time (by `build`) and during
hot-reload when parameters change on a surviving module. `update_validated_parameters`
receives a pre-validated map and can assume all keys are declared and values are
correctly typed and within bounds.

## Parameter types

| DSL type | `ParameterKind` | `ParameterValue` |
|---|---|---|
| float, Hz, seconds | `Float { min, max, default }` | `Float(f32)` |
| integer | `Int { min, max, default }` | `Int(i64)` |
| boolean | `Bool { default }` | `Bool(bool)` |
| string enum | `Enum { variants, default }` | `Enum(&'static str)` |
| array of strings | `Array { default, length }` | `Array(Box<[String]>)` |

All descriptor fields are `&'static str` or `&'static [...]` — the descriptor
itself never allocates. `ParameterValue::Enum` holds a `&'static str` by
convention (the variant string is a compile-time constant).

## Plan activation

When the audio thread swaps in a new plan, it calls:

1. `set_ports(&inputs, &outputs)` for every module whose port connectivity
   changed — delivers resolved `MonoInput`/`MonoOutput` etc. objects with correct
   `cable_idx` values.
2. `update_validated_parameters(params)` for every module whose parameters
   changed, without reinitialising it.

Both calls happen in between samples (at a safe point between `tick()` calls),
so modules must not allocate or block inside either method.
