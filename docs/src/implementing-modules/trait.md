# The Module trait

All audio modules implement `patches_core::Module`. The trait is defined in
`patches-core/src/modules/module.rs`.

```rust
pub trait Module: Send {
    // ── Descriptor ────────────────────────────────────────────────────────────
    fn describe(shape: &ModuleShape) -> ModuleDescriptor where Self: Sized;
    fn descriptor(&self) -> &ModuleDescriptor;

    // ── Construction ──────────────────────────────────────────────────────────
    fn prepare(env: &AudioEnvironment, descriptor: ModuleDescriptor, id: InstanceId) -> Self
        where Self: Sized;
    fn update_validated_parameters(&mut self, params: &ParameterMap);
    fn update_parameters(&mut self, params: &ParameterMap) -> Result<(), BuildError> { /* default */ }
    fn build(env: &AudioEnvironment, shape: &ModuleShape, params: &ParameterMap, id: InstanceId)
        -> Result<Self, BuildError> where Self: Sized { /* default */ }

    // ── Identity ──────────────────────────────────────────────────────────────
    fn instance_id(&self) -> InstanceId;

    // ── Runtime ───────────────────────────────────────────────────────────────
    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) { /* default no-op */ }
    fn process(&mut self, pool: &mut CablePool<'_>);

    // ── Opt-in capabilities ───────────────────────────────────────────────────
    fn as_any(&self) -> &dyn std::any::Any;
    fn as_midi_receiver(&mut self) -> Option<&mut dyn ReceivesMidi> { None }
}
```

## Method-by-method guide

### `describe(shape) -> ModuleDescriptor`

A static method — called on the type, not an instance. Returns the descriptor
for this module at the given shape. Build it with the `ModuleDescriptor` builder:

```rust
fn describe(shape: &ModuleShape) -> ModuleDescriptor {
    ModuleDescriptor::new("MyModule", shape.clone())
        .mono_in("signal")          // adds input port "signal/0"
        .mono_in("cv")              // adds input port "cv/0"
        .mono_out("out")            // adds output port "out/0"
        .float_param("gain", 0.0, 2.0, 1.0)   // name, min, max, default
        .enum_param("mode", &["add", "mul"], "add")
}
```

Port and parameter names must be `&'static str` (string literals). The order
you add ports determines their index in the slices passed to `set_ports` and
used in `process`.

Builder methods for ports:

| Method | Adds |
|---|---|
| `.mono_in("name")` | One `MonoInput` port at index 0 |
| `.mono_in_multi("name", n)` | `n` `MonoInput` ports at indices 0..n |
| `.poly_in("name")` | One `PolyInput` port |
| `.poly_in_multi("name", n)` | `n` `PolyInput` ports |
| `.mono_out("name")` | One `MonoOutput` port |
| `.mono_out_multi("name", n)` | `n` `MonoOutput` ports |
| `.poly_out("name")` | One `PolyOutput` port |
| `.poly_out_multi("name", n)` | `n` `PolyOutput` ports |

Builder methods for parameters:

| Method | Adds |
|---|---|
| `.float_param("name", min, max, default)` | One float parameter |
| `.float_param_multi("name", n, min, max, default)` | `n` indexed float params |
| `.int_param("name", min, max, default)` | One integer parameter |
| `.int_param_multi(...)` | `n` indexed integer params |
| `.bool_param("name", default)` | One boolean |
| `.enum_param("name", &["a", "b"], "a")` | One string enum |
| `.array_param("name", &[], max_length)` | Variable-length string array |
| `.sink()` | Marks module as the audio output sink |

### `prepare(env, descriptor, instance_id) -> Self`

Allocates and returns a new instance. Store `env.sample_rate` and
`env.poly_voices` if needed. Store `descriptor` and `instance_id`. Set all port
fields to their defaults (`MonoInput::default()`, etc.) — they will be properly
filled by the first `set_ports` call before `process` runs.

This method is infallible. Do not perform parameter validation here.

### `update_validated_parameters(&mut self, params: &ParameterMap)`

Apply parameters to the instance's fields. The params have already been validated
against the descriptor (types, ranges, and enum variants are guaranteed correct).
Extract values with:

```rust
use patches_core::parameter_map::ParameterValue;

// fetch a float at index 0 (the default index for single-value params)
if let Some(ParameterValue::Float(v)) = params.get("gain", 0) {
    self.gain = *v;
}
// fetch an enum
if let Some(ParameterValue::Enum(s)) = params.get("mode", 0) {
    self.mode = match *s { "mul" => Mode::Mul, _ => Mode::Add };
}
// fetch an indexed float (e.g. level[2])
if let Some(ParameterValue::Float(v)) = params.get("level", 2) {
    self.levels[2] = *v;
}
```

### `descriptor(&self) -> &ModuleDescriptor`

Return a reference to the descriptor stored in the struct. Typically:

```rust
fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
```

### `instance_id(&self) -> InstanceId`

Return the id stored at construction. Typically:

```rust
fn instance_id(&self) -> InstanceId { self.instance_id }
```

### `set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort])`

Called by the engine when the patch topology changes. Store the resolved port
objects for use in `process`. The slice positions correspond to the order ports
were added in `describe`.

```rust
fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
    self.in_signal = MonoInput::from_ports(inputs, 0);
    self.in_cv     = MonoInput::from_ports(inputs, 1);
    self.out_audio = MonoOutput::from_ports(outputs, 0);
}
```

Each `from_ports` call extracts the port at the given position and panics if the
type doesn't match — which would indicate a mismatch between `describe` and
`set_ports`. The planner guarantees types match; a panic here is a bug in the
module implementation.

**Must not allocate, block, or perform I/O.** May be called on the audio thread.

### `process(&mut self, pool: &mut CablePool<'_>)`

Called once per sample. Read inputs and write outputs using the port objects
stored in `set_ports`:

```rust
fn process(&mut self, pool: &mut CablePool<'_>) {
    let signal = pool.read_mono(&self.in_signal);
    let cv     = pool.read_mono(&self.in_cv);
    pool.write_mono(&self.out_audio, signal * cv);
}
```

`CablePool` methods:

| Method | Description |
|---|---|
| `pool.read_mono(&MonoInput) -> f32` | Read mono value, scaled by `input.scale` |
| `pool.read_poly(&PolyInput) -> [f32; 16]` | Read all 16 voices, scaled |
| `pool.write_mono(&MonoOutput, f32)` | Write mono value |
| `pool.write_poly(&PolyOutput, [f32; 16])` | Write all 16 voices |

**Must not allocate, block, or perform I/O.** Called at audio rate (typically
44,100 times per second). Pre-compute or look up all values; avoid branches that
touch heap memory.

### `as_any(&self) -> &dyn std::any::Any`

Required for downcasting in tests. Always implemented as:

```rust
fn as_any(&self) -> &dyn std::any::Any { self }
```

### Optional: `as_midi_receiver`

If the module should receive MIDI events, implement `ReceivesMidi` and override:

```rust
fn as_midi_receiver(&mut self) -> Option<&mut dyn ReceivesMidi> { Some(self) }
```

The planner uses this during plan construction to build the MIDI dispatch list.
Modules that return `None` (the default) are never called for MIDI events.

### Marking a module as the audio sink

A module that is the final audio output marks itself in the descriptor, not via
a trait method:

```rust
fn describe(shape: &ModuleShape) -> ModuleDescriptor {
    ModuleDescriptor::new("AudioOut", shape.clone())
        .mono_in("in_left")
        .mono_in("in_right")
        .sink()   // ← marks this module as the sink node
}
```

The planner uses `ModuleDescriptor::is_sink` to detect the sink. The output
values themselves are delivered via the `AUDIO_OUT_L` / `AUDIO_OUT_R` backplane
slots — the module writes to those slots from `process`, and the audio callback
reads them directly after each tick. There is no separate trait to implement.
