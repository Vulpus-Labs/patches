# Implementing modules

This chapter covers everything needed to add a new module to Patches: the `Module` trait, the descriptor builder, a worked example, registration, and testing.

## The Module trait

All audio modules implement `patches_core::Module` (defined in `patches-core/src/modules/module.rs`). The trait has a small surface:

```rust
pub trait Module: Send {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor where Self: Sized;
    fn prepare(env: &AudioEnvironment, descriptor: ModuleDescriptor, id: InstanceId) -> Self
        where Self: Sized;
    fn descriptor(&self) -> &ModuleDescriptor;
    fn instance_id(&self) -> InstanceId;
    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]);
    fn update_validated_parameters(&mut self, params: &ParameterMap);
    fn process(&mut self, pool: &mut CablePool<'_>);
    fn as_any(&self) -> &dyn std::any::Any;

    // Optional:
    fn as_midi_receiver(&mut self) -> Option<&mut dyn ReceivesMidi> { None }
}
```

The lifecycle is: `describe` (static, at registration) → `prepare` (construct instance) → `set_ports` (called when topology changes) → `update_validated_parameters` (called when parameters change) → `process` (called once per sample, at audio rate).

### describe

A static method — called on the type, not an instance. Returns the `ModuleDescriptor` for this module at the given shape. Built using the descriptor builder:

```rust
fn describe(shape: &ModuleShape) -> ModuleDescriptor {
    ModuleDescriptor::new("MyModule", shape.clone())
        .mono_in("signal")
        .mono_in("cv")
        .mono_out("out")
        .float_param("gain", 0.0, 2.0, 1.0)
        .enum_param("mode", &["add", "mul"], "add")
}
```

Port and parameter names are `&'static str`. The order you add ports determines their positional index in `set_ports`.

### prepare

Allocates and returns a new instance. Store the descriptor, instance ID, and `env.sample_rate` if needed. Initialise all port fields to their defaults (`MonoInput::default()`, etc.) — they will be set properly by the first `set_ports` call before `process` runs.

This method is infallible.

### set_ports

Called when the patch topology changes. The slice positions correspond to the order ports were declared in `describe`. Extract ports with `MonoInput::from_ports(inputs, index)`.

Must not allocate, block, or perform I/O — it may be called on the audio thread.

### update_validated_parameters

Apply parameter values to the instance's fields. The map has already been validated against the descriptor, so types and ranges are guaranteed correct. Parameters not present in the map are unchanged — leave them at their current values.

```rust
fn update_validated_parameters(&mut self, params: &ParameterMap) {
    if let Some(ParameterValue::Float(v)) = params.get("gain", 0) {
        self.gain = *v;
    }
}
```

### process

Called once per sample at audio rate (typically 44,100 times per second). Read inputs and write outputs through the cable pool:

```rust
fn process(&mut self, pool: &mut CablePool<'_>) {
    let signal = pool.read_mono(&self.in_signal);
    let cv = pool.read_mono(&self.in_cv);
    pool.write_mono(&self.out_audio, signal * cv);
}
```

**Must not allocate, block, or perform I/O.** Pre-compute or look up all values. Avoid branches that touch heap memory.

### descriptor, instance_id, as_any

Boilerplate — return the stored descriptor, the stored ID, and `self` respectively.

## Descriptor builder reference

### Port methods

| Method | Adds |
|--------|------|
| `.mono_in("name")` | One `MonoInput` port |
| `.mono_in_multi("name", n)` | `n` `MonoInput` ports at indices 0..n |
| `.poly_in("name")` | One `PolyInput` port |
| `.poly_in_multi("name", n)` | `n` `PolyInput` ports |
| `.mono_out("name")` | One `MonoOutput` port |
| `.mono_out_multi("name", n)` | `n` `MonoOutput` ports |
| `.poly_out("name")` | One `PolyOutput` port |
| `.poly_out_multi("name", n)` | `n` `PolyOutput` ports |

### Parameter methods

| Method | Adds |
|--------|------|
| `.float_param("name", min, max, default)` | Float parameter |
| `.float_param_multi("name", n, min, max, default)` | `n` indexed float params |
| `.int_param("name", min, max, default)` | Integer parameter |
| `.bool_param("name", default)` | Boolean parameter |
| `.enum_param("name", &["a", "b"], "a")` | String enum parameter |
| `.array_param("name", &[], max_length)` | Variable-length string array |
| `.sink()` | Marks module as the audio output sink |

## Worked example: Gain

A module with one audio input, one CV input, a `gain` float parameter, a `clip` boolean parameter, and one output.

### Struct

```rust
// patches-modules/src/gain.rs

use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId,
    Module, ModuleDescriptor, ModuleShape, MonoInput, MonoOutput, OutputPort,
};
use patches_core::parameter_map::{ParameterMap, ParameterValue};

pub struct Gain {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    in_signal: MonoInput,
    in_cv: MonoInput,
    out_audio: MonoOutput,
    gain: f32,
    clip: bool,
}
```

### Implementation

```rust
impl Module for Gain {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("Gain", shape.clone())
            .mono_in("in")
            .mono_in("cv")
            .mono_out("out")
            .float_param("gain", 0.0, 4.0, 1.0)
            .bool_param("clip", false)
    }

    fn prepare(
        _env: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
    ) -> Self {
        Self {
            instance_id,
            descriptor,
            in_signal: MonoInput::default(),
            in_cv: MonoInput::default(),
            out_audio: MonoOutput::default(),
            gain: 1.0,
            clip: false,
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }
    fn as_any(&self) -> &dyn std::any::Any { self }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_signal = MonoInput::from_ports(inputs, 0);  // matches "in"
        self.in_cv     = MonoInput::from_ports(inputs, 1);  // matches "cv"
        self.out_audio = MonoOutput::from_ports(outputs, 0); // matches "out"
    }

    fn update_validated_parameters(&mut self, params: &ParameterMap) {
        if let Some(ParameterValue::Float(v)) = params.get("gain", 0) {
            self.gain = *v;
        }
        if let Some(ParameterValue::Bool(v)) = params.get("clip", 0) {
            self.clip = *v;
        }
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let signal = pool.read_mono(&self.in_signal);
        let cv     = pool.read_mono(&self.in_cv);
        let mut out = signal * cv * self.gain;
        if self.clip {
            out = out.clamp(-1.0, 1.0);
        }
        pool.write_mono(&self.out_audio, out);
    }
}
```

Port order in `describe` must match the positional indices in `set_ports`. Here `"in"` is input index 0, `"cv"` is input index 1, and `"out"` is output index 0.

### Registration

In `patches-modules/src/lib.rs`:

```rust
pub mod gain;
pub use gain::Gain;
```

And in `default_registry()`:

```rust
r.register::<Gain>();
```

The DSL name comes from the string passed to `ModuleDescriptor::new` — here `"Gain"`. Users write `module g : Gain { gain: 0.5, clip: true }`.

## Testing with ModuleHarness

`ModuleHarness` from `patches_core::test_support` provides a single-module test fixture with a cable pool, named-port accessors, and automatic ping-pong management.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::test_support::{assert_nearly, ModuleHarness, params};

    #[test]
    fn multiplies_signal_by_cv_and_gain() {
        let mut h = ModuleHarness::build::<Gain>(params!["gain" => 2.0_f32]);
        h.set_mono("in", 0.5);
        h.set_mono("cv", 0.8);
        h.tick();
        assert_nearly!(0.8, h.read_mono("out"));  // 0.5 * 0.8 * 2.0
    }

    #[test]
    fn clip_clamps_output() {
        let mut h = ModuleHarness::build::<Gain>(params!["gain" => 4.0_f32, "clip" => true]);
        h.set_mono("in", 1.0);
        h.set_mono("cv", 1.0);
        h.tick();
        assert_nearly!(1.0, h.read_mono("out"));  // 4.0 clamped to 1.0
    }
}
```

`params!` builds a `ParameterMap` from key-value pairs. `set_mono` writes a value to a named input port. `tick` runs one sample through the module. `read_mono` reads a named output port.

## Checklist

Before submitting a new module:

- `describe` declares all ports and parameters with correct names and types
- Port order in `describe` matches `set_ports` positional indices
- `prepare` initialises all port fields to their defaults
- `update_validated_parameters` handles all declared parameters
- `process` does not allocate, block, or perform I/O
- Module registered in `default_registry()`
- Unit tests with `ModuleHarness` cover the main behaviours
- `cargo clippy` and `cargo test` pass
- Doc comment on the struct follows the [module documentation standard](https://github.com/anthropics/patches/blob/main/CLAUDE.md)
