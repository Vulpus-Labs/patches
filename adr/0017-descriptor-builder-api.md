# ADR 0017 — ModuleDescriptor builder API

**Date:** 2026-03-18
**Status:** Accepted

## Context

Every `Module::describe()` implementation constructs a `ModuleDescriptor` using struct
literals with manually assigned indices. For a module with three inputs, two outputs, and
two parameters, this is typically 25–40 lines of noise that adds no information beyond what
the names and kinds already imply.

The redundancy is threefold:

**Manual positional indices.** Each `PortDescriptor` carries an `index: u32` that the author
must assign correctly. For modules with a single port per name the value is always `0`; for
multi-port modules (e.g. a mixer) the indices must be enumerated by hand and kept in sync
with `set_ports()`. There is no compile-time check that `describe()` and `set_ports()` agree.

**Struct literal noise.** Every port and parameter requires a fully-qualified struct literal
(`PortDescriptor { name: "in", index: 0, kind: CableKind::Mono }`) even when the name and
kind are the only non-trivial fields.

**Multi-port repetition.** Shape-driven modules (mixers, sequencers) generate one
`PortDescriptor` or `ParameterDescriptor` per channel. This forces either a manual
enumeration that must be updated when the channel count changes, or a helper `for` loop that
adds local noise.

The existing types (`PortDescriptor`, `ParameterDescriptor`, `ModuleDescriptor`) are
correct and must remain unchanged — they are used throughout the planner, graph, and DSL
compiler. The issue is solely the authoring ergonomics of constructing them in `describe()`.

## Decision

Add a builder API to `ModuleDescriptor` in `patches-core`. The API is purely additive: all
existing struct literal construction remains valid.

### Single-port methods

```rust
impl ModuleDescriptor {
    pub fn new(name: &'static str, shape: ModuleShape) -> Self;

    pub fn mono_in(self, name: &'static str) -> Self;
    pub fn mono_out(self, name: &'static str) -> Self;
    pub fn poly_in(self, name: &'static str) -> Self;
    pub fn poly_out(self, name: &'static str) -> Self;

    pub fn float_param(self, name: &'static str, min: f32, max: f32, default: f32) -> Self;
    pub fn int_param(self, name: &'static str, min: i64, max: i64, default: i64) -> Self;
    pub fn bool_param(self, name: &'static str, default: bool) -> Self;
    pub fn enum_param(self, name: &'static str, variants: &'static [&'static str], default: &'static str) -> Self;
    pub fn array_param(self, name: &'static str, default: &'static [&'static str], length: usize) -> Self;

    pub fn sink(self) -> Self;
}
```

Single-port methods always use `index: 0`. A `mono_in("in")` call is equivalent to:

```rust
inputs.push(PortDescriptor { name: "in", index: 0, kind: CableKind::Mono });
```

### Multi-port methods

```rust
    pub fn mono_in_multi(self, name: &'static str, count: u32) -> Self;
    pub fn mono_out_multi(self, name: &'static str, count: u32) -> Self;
    pub fn poly_in_multi(self, name: &'static str, count: u32) -> Self;
    pub fn poly_out_multi(self, name: &'static str, count: u32) -> Self;

    pub fn float_param_multi(self, name: &'static str, count: usize, min: f32, max: f32, default: f32) -> Self;
    pub fn int_param_multi(self, name: &'static str, count: usize, min: i64, max: i64, default: i64) -> Self;
    pub fn bool_param_multi(self, name: &'static str, count: usize, default: bool) -> Self;
    pub fn enum_param_multi(self, name: &'static str, count: usize, variants: &'static [&'static str], default: &'static str) -> Self;
```

`mono_in_multi("in", n)` pushes `n` entries with indices `0..n`, equivalent to:

```rust
for i in 0..n {
    inputs.push(PortDescriptor { name: "in", index: i, kind: CableKind::Mono });
}
```

### Example — before and after

**Vca** (13 lines → 4):

```rust
// Before
ModuleDescriptor {
    module_name: "Vca",
    shape: shape.clone(),
    inputs: vec![
        PortDescriptor { name: "in", index: 0, kind: CableKind::Mono },
        PortDescriptor { name: "cv", index: 0, kind: CableKind::Mono },
    ],
    outputs: vec![PortDescriptor { name: "out", index: 0, kind: CableKind::Mono }],
    parameters: vec![],
    is_sink: false,
}

// After
ModuleDescriptor::new("Vca", shape.clone())
    .mono_in("in")
    .mono_in("cv")
    .mono_out("out")
```

**Mixer** with `shape.channels = n` (26 lines → 8, and correct for any `n`):

```rust
ModuleDescriptor::new("Mixer", shape.clone())
    .mono_in_multi("in", n)
    .mono_in_multi("gain_mod", n)
    .mono_in_multi("pan_mod", n)
    .mono_out("out_l")
    .mono_out("out_r")
    .float_param_multi("gain", n as usize, 0.0, 1.2, 1.0)
    .float_param_multi("pan",  n as usize, -1.0, 1.0, 0.0)
    .bool_param_multi("mute",  n as usize, false)
    .bool_param_multi("solo",  n as usize, false)
```

## Consequences

### Benefits

- **Less noise in `describe()`.** The information density of a descriptor definition
  increases substantially. Port names, kinds, and parameter ranges are visible at a glance
  without parsing struct field names.

- **Multi-port descriptors are robust to shape changes.** `_multi` methods with a
  `shape.channels`-derived count generate the correct number of ports for any shape, removing
  the class of bug where a port count is hardcoded.

- **No new dependencies.** The builder is a pure addition to `patches-core` with no external
  crates. Each builder method is a trivial `push` onto the existing `Vec` fields.

- **Fully backwards-compatible.** All existing struct literal construction compiles unchanged.
  Migration is optional and can be done incrementally.

### Costs

- **The `index` field on `PortDescriptor` is now implicit for single-port use.** Authors using
  the builder cannot supply a non-zero index to a single-port method — they must use
  `_multi` even for a single port at index 1. This is an intentional constraint: a lone port
  at index 1 with no port at index 0 would be a descriptor error. If such a case arises it
  should be expressed by providing two ports, making the gap explicit.

- **Descriptor validation is still runtime.** The builder does not enforce invariants such as
  "indices within a name group are contiguous starting at 0". That validation remains the
  responsibility of `validate_parameters` and graph construction. A future ADR may address
  compile-time descriptor validation via a proc-macro derive.

## Alternatives considered

### Proc-macro derive

A `#[derive(ModuleDescriptor)]` macro could generate `describe()` entirely from struct field
annotations, eliminating `set_ports()` boilerplate as well. Rejected for this ADR because it
requires a new crate dependency (`syn`, `quote`), the implementation complexity is high
relative to the gain, and the `set_ports()` problem (field assignment order vs descriptor
order) is a separate concern. Revisit if descriptor and `set_ports()` drift continues to
cause bugs.

### Dedicated `DescriptorBuilder` struct

A separate `DescriptorBuilder` type could enforce a build-then-seal pattern, preventing
accidental mutation after construction. Rejected: `ModuleDescriptor` is already immutable in
practice (no mutation methods exist), and a separate type adds API surface without a
meaningful safety gain.

### Keep struct literals as-is

Acceptable given that the structs are simple and the pattern is established. Rejected because
multi-port modules suffer most and the `_multi` variants genuinely eliminate a class of
shape-mismatch bug.
