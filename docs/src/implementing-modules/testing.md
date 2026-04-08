# Testing modules

## ModuleHarness

`ModuleHarness` (from `patches_core::test_support`) is a single-module test
fixture. It owns a module, a cable pool, and named-port accessors. It eliminates
the boilerplate of pool sizing, `set_ports` setup, ping-pong management, and
`CableValue` unwrapping from test bodies.

All ports are marked `connected: true` by default.

### Construction

```rust
use patches_core::test_support::{ModuleHarness, params};

// Default env (44100 Hz, 16 voices), default shape (channels: 0, length: 0)
let mut h = ModuleHarness::build::<MyModule>(&[]);

// With parameters
let mut h = ModuleHarness::build::<MyModule>(params!["gain" => 1.5_f32]);

// With a non-default shape (e.g. a variable-channel mixer)
let mut h = ModuleHarness::build_with_shape::<Sum>(
    &[],
    ModuleShape { channels: 3, length: 0 },
);

// With a custom AudioEnvironment (e.g. for frequency/time-dependent modules)
let mut h = ModuleHarness::build_with_env::<Glide>(
    params!["time" => 0.05_f32],
    AudioEnvironment { sample_rate: 48000.0, poly_voices: 16 },
);
```

### Setting inputs

```rust
h.set_mono("in", 0.5);          // write to mono input "in" (index 0)
h.set_mono_at("in", 1, 0.3);   // write to indexed mono input "in/1"
h.set_poly("voct", [0.0; 16]); // write to poly input "voct"
```

Values written with `set_mono` / `set_poly` persist across multiple `tick()`
calls until overwritten.

### Ticking

```rust
h.tick();               // process one sample
h.tick().tick().tick(); // chain multiple ticks
```

### Reading outputs

```rust
let v = h.read_mono("out");          // read mono output "out" (index 0)
let v = h.read_mono_at("out", 1);   // read indexed output "out/1"
let v = h.read_poly("out");          // read poly output as [f32; 16]
let v = h.read_poly_voice("out", 3); // read voice 3 from poly output
```

### Batch helpers

```rust
// Run 100 ticks, collect output named "out" as Vec<f32>
let samples = h.run_mono(100, "out");

// Run with input fed from a slice (cycled if shorter than tick count)
let samples = h.run_mono_mapped(44100, "in", &[0.5, -0.5], "out");
```

### Connectivity

```rust
h.disconnect_input("cv");           // set connected=false on "cv/0"
h.disconnect_input_at("in", 1);    // set connected=false on "in/1"
h.disconnect_output("out");         // set connected=false on "out/0"
h.disconnect_all_inputs();
```

`set_ports` is called immediately after each disconnect, so the module sees the
updated connectivity before the next `tick()`.

### Parameter updates (hot-reload simulation)

```rust
h.update_validated_parameters(params!["gain" => 2.0_f32]);
```

Simulates a hot-reload parameter change — calls `update_validated_parameters`
directly, without reinitialising the module.

## The `params!` macro

Builds a `&[(&str, ParameterValue)]` slice from `key => value` pairs, inferring
the `ParameterValue` variant from the Rust literal type:

```rust
params![
    "gain"      => 1.5_f32,    // ParameterValue::Float
    "voices"    => 4_i64,      // ParameterValue::Int
    "active"    => true,       // ParameterValue::Bool
    "waveform"  => "sine",     // ParameterValue::Enum
]
```

Note the type suffixes: `f32` for floats, `i64` for integers. Plain integer
literals (`4`) infer as `i32`, which is not supported — always write `4_i64`.

## Assertion macros

### `assert_nearly!(expected, actual)`

Checks that `actual` is within a relative epsilon of `expected`. The tolerance
scales with `max(|expected|, 1.0) × f32::EPSILON`, so it works correctly for
values well above or below 1.0 (e.g. comparing frequencies at 440 Hz).

```rust
assert_nearly!(440.0, computed_frequency);
assert_nearly!(0.5, h.read_mono("out"));
```

### `assert_within!(expected, actual, delta)`

Checks that `|actual - expected| < delta`. Use when the tolerance is a known
physical quantity rather than a relative epsilon:

```rust
// Phase should be within half a semitone
assert_within!(expected_phase, h.read_mono("out"), 0.005);
```

## Example: testing connectivity behaviour

A module can skip work on disconnected outputs to save CPU. Here is how to test
that:

```rust
#[test]
fn skips_computation_when_output_disconnected() {
    let mut h = ModuleHarness::build::<Osc>(&[]);
    h.disconnect_output("triangle");
    h.init_pool(CableValue::Mono(99.0)); // sentinel
    h.tick();
    // If the module correctly skips, it writes nothing to "triangle".
    // The pool slot should still hold the sentinel value.
    // (Use h.pool_slot(idx) for direct slot inspection.)
}
```

`h.init_pool(value)` fills all pool slots with a sentinel before the first tick,
so you can distinguish "module wrote zero" from "module skipped the write".
