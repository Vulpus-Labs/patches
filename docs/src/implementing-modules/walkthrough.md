# Worked example: a gain module

This page walks through implementing a complete module from scratch. We'll build
`Gain` — a simple module with one audio input, one control input (gain amount),
a `clip` boolean parameter, and one audio output.

## 1. Plan the interface

Decide on ports and parameters before writing any code:

| Port / parameter | Kind | Default | Notes |
|---|---|---|---|
| `in` | mono input | — | Audio signal |
| `cv` | mono input | — | Gain CV (0–2) |
| `out` | mono output | — | `in × cv`, optionally clipped |
| `gain` | float param | 1.0 | Static gain multiplied with `cv` |
| `clip` | bool param | false | If true, clamp output to [-1, 1] |

## 2. Write the struct

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
    // Port fields — populated by set_ports, defaulted in prepare
    in_signal: MonoInput,
    in_cv: MonoInput,
    out_audio: MonoOutput,
    // Parameter fields — populated by update_validated_parameters
    gain: f32,
    clip: bool,
}
```

## 3. Implement `describe`

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
```

Port order in `describe` must match the positional indices used in `set_ports`.
Here `in` is index 0, `cv` is index 1, and `out` is index 0.

## 4. Implement `prepare`

```rust
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
```

All port fields use `Default`, which points them at the null slots (reads return
zero, writes go nowhere) until `set_ports` is called.

## 5. Implement `update_validated_parameters`

```rust
    fn update_validated_parameters(&mut self, params: &ParameterMap) {
        if let Some(ParameterValue::Float(v)) = params.get("gain", 0) {
            self.gain = *v;
        }
        if let Some(ParameterValue::Bool(v)) = params.get("clip", 0) {
            self.clip = *v;
        }
    }
```

Parameters not present in `params` (i.e. unchanged in this update) should be
left at their current values. The `if let` / `get` pattern handles this naturally.

## 6. Implement the boilerplate accessors

```rust
    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }
    fn as_any(&self) -> &dyn std::any::Any { self }
```

## 7. Implement `set_ports`

```rust
    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_signal = MonoInput::from_ports(inputs, 0);
        self.in_cv     = MonoInput::from_ports(inputs, 1);
        self.out_audio = MonoOutput::from_ports(outputs, 0);
    }
```

## 8. Implement `process`

```rust
    fn process(&mut self, pool: &mut CablePool<'_>) {
        let signal = pool.read_mono(&self.in_signal);
        let cv     = pool.read_mono(&self.in_cv);
        let mut out = signal * cv * self.gain;
        if self.clip {
            out = out.clamp(-1.0, 1.0);
        }
        pool.write_mono(&self.out_audio, out);
    }
} // end impl Module for Gain
```

## 9. Register the module

Open `patches-modules/src/lib.rs` and add:

```rust
pub mod gain;
pub use gain::Gain;
```

Then in `default_registry()`:

```rust
r.register::<Gain>();
```

The DSL name is taken from the string passed to `ModuleDescriptor::new` — here
`"Gain"`. Users can now write `module g : Gain { gain: 0.5, clip: true }` in
their patches.

## 10. Write tests

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
        // 0.5 × 0.8 × 2.0 = 0.8
        assert_nearly!(0.8, h.read_mono("out"));
    }

    #[test]
    fn clip_clamps_to_minus_one_plus_one() {
        let mut h = ModuleHarness::build::<Gain>(params!["gain" => 4.0_f32, "clip" => true]);
        h.set_mono("in", 1.0);
        h.set_mono("cv", 1.0);
        h.tick();
        assert_nearly!(1.0, h.read_mono("out"));
    }

    #[test]
    fn without_clip_allows_values_above_one() {
        let mut h = ModuleHarness::build::<Gain>(params!["gain" => 4.0_f32]);
        h.set_mono("in", 1.0);
        h.set_mono("cv", 1.0);
        h.tick();
        assert_nearly!(4.0, h.read_mono("out"));
    }
}
```

## Checklist

Before opening a PR for a new module:

- [ ] `describe` declares all ports and parameters with correct names and types
- [ ] Port order in `describe` matches `set_ports` positional indices
- [ ] `prepare` initialises all port fields to their defaults
- [ ] `update_validated_parameters` handles all declared parameters
- [ ] `process` does not allocate, block, or perform I/O
- [ ] Module registered in `default_registry()`
- [ ] Unit tests with `ModuleHarness` cover the main behaviours
- [ ] `cargo clippy` and `cargo test` pass
