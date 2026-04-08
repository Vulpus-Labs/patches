# ADR 0018 — Module test harness

**Date:** 2026-03-18
**Status:** Accepted

## Context

Unit tests for module implementations share a large amount of structural setup that is
unrelated to the behaviour under test. A typical test in `patches-modules` must:

1. Create a `Registry`, register the concrete type, and call `r.create(...)` with a
   hardcoded module name string, an `AudioEnvironment` literal, a `ModuleShape` literal,
   and a `ParameterMap`.
2. Allocate a flat pool (`Vec<[CableValue; 2]>`) sized to cover all ports plus the
   ping-pong slot.
3. Construct `InputPort` and `OutputPort` vecs with cable indices manually mapped to
   descriptor positions, duplicating the descriptor layout.
4. Call `module.set_ports(...)`.
5. Write input values into the pool by numeric cable index, advance the ping-pong `wi`
   index manually, call `module.process(...)`, then extract output values by index and
   unwrap the `CableValue` variant.

This infrastructure is repeated in every test function or extracted into per-module helpers
(`make_vca`, `set_ports_for_test`, `make_pool`) that themselves differ only in the port
count and cable indices. The result is a high ratio of scaffolding to expressed intent.

Specific pain points:

- **Pool sizing and index arithmetic are implicit.** There is no single source of truth for
  which cable index maps to which port. Comments such as `// 0=in, 1=cv, 2=out` are the
  only documentation, and they drift.
- **Ping-pong management leaks into test bodies.** Tests that run multiple ticks must
  manually alternate `wi` between 0 and 1 (`let wi = i % 2`).
- **`CableValue` unwrapping is repetitive.** Every output read is
  `if let CableValue::Mono(v) = pool[idx][wi] { ... } else { panic!(...) }`.
- **Multi-tick stream tests have no standard shape.** Each test that runs N ticks and
  collects an output array invents its own loop.
- **Assertion noise.** `assert!((expected - actual).abs() < f32::EPSILON)` is both verbose
  and subtly wrong for values far from 1.0 (see below).

## Decision

### `ModuleHarness`

Add `ModuleHarness` to `patches-core` behind a `test-support` Cargo feature (also enabled
in `#[cfg(test)]`). The harness owns the module instance and its cable pool, derives port
assignments automatically from the descriptor, and advances the ping-pong index internally.

```rust
pub struct ModuleHarness {
    module:     Box<dyn Module>,
    pool:       Vec<[CableValue; 2]>,
    input_map:  HashMap<(&'static str, u32), usize>,  // (name, index) → cable_idx
    output_map: HashMap<(&'static str, u32), usize>,
    wi:         usize,
}

impl ModuleHarness {
    /// Construct a harness for module type `M` with the given parameters.
    /// All ports are connected by default.
    pub fn build<M: Module>(params: &[(&str, ParameterValue)]) -> Self;

    /// Set a mono input value for the next tick.
    pub fn set_mono(&mut self, name: &str, value: f32);
    /// Set a mono input by name and explicit index (multi-port modules).
    pub fn set_mono_at(&mut self, name: &str, index: u32, value: f32);

    /// Advance one sample, returning &mut self for chaining.
    pub fn tick(&mut self) -> &mut Self;

    /// Read a mono output from the most recently completed tick.
    pub fn read_mono(&self, name: &str) -> f32;
    /// Read a mono output by name and explicit index.
    pub fn read_mono_at(&self, name: &str, index: u32) -> f32;

    /// Run `n` ticks and collect one named output as a Vec<f32>.
    pub fn run_mono(&mut self, ticks: usize, output: &str) -> Vec<f32>;

    /// Run `n` ticks, feeding `inputs` (cycled if shorter than `ticks`) into a
    /// named input each tick, and collect a named output as Vec<f32>.
    pub fn run_mono_mapped(
        &mut self,
        ticks:  usize,
        input:  &str,
        inputs: &[f32],
        output: &str,
    ) -> Vec<f32>;
}
```

The harness reads the descriptor at construction time to assign cable indices in descriptor
order (inputs first, then outputs), allocates a pool large enough for all cables plus the
ping-pong slot, and calls `set_ports` once. All ports are marked `connected: true` by
default; a `disconnect` method can be added if a test needs to check connectivity-dependent
behaviour.

Poly variants (`set_poly`, `read_poly`, `run_poly_mapped`) follow the same pattern.

### `params!` macro

Building `&[(&str, ParameterValue)]` for `ModuleHarness::build` is verbose for the common
case of float, bool, and enum parameters. A declarative macro infers the `ParameterValue`
variant from the literal type:

```rust
// f32 literal → ParameterValue::Float
// bool literal → ParameterValue::Bool
// &str literal → ParameterValue::Enum
// i64 suffix   → ParameterValue::Int
params![
    "frequency" => 440.0_f32,
    "fm_type"   => "linear",
    "saturate"  => false,
]
```

### Assertion macros

Two assertion macros replace the inline tolerance checks:

```rust
/// Assert that actual is within a relative epsilon of expected.
/// Tolerance scales with max(|expected|, 1.0) so the check is correct
/// for values well above or below 1.0.
macro_rules! assert_nearly {
    ($expected:expr, $actual:expr) => { ... };
}

/// Assert that actual is within an absolute delta of expected.
/// Use when the domain tolerance is known (Hz, dB, seconds).
macro_rules! assert_within {
    ($expected:expr, $actual:expr, $delta:expr) => { ... };
}
```

`assert_nearly` uses `(expected - actual).abs() < f32::EPSILON * expected.abs().max(1.0)`.
The scaling factor corrects the common mistake of using raw `f32::EPSILON` for values near
e.g. 440 Hz, where accumulated float error routinely exceeds `f32::EPSILON` even for
numerically correct results.

`assert_within` is absolute and explicit — appropriate for domain quantities where the
caller knows the meaningful precision (e.g. ±0.5 Hz for oscillator frequency, ±0.001 for
filter coefficient checks).

Both macros produce failure messages that include expected, actual, and the tolerance used.

### Placement

All three items live in `patches-core` under `src/test_support/`:

```
patches-core/src/test_support/
    mod.rs          — re-exports; gated on cfg(any(test, feature = "test-support"))
    harness.rs      — ModuleHarness
    macros.rs       — assert_nearly!, assert_within!, params!
```

The `test-support` feature is added to `patches-core/Cargo.toml`. Crates that need it in
non-test builds (none currently) can enable it explicitly; `patches-modules` and
`patches-integration-tests` already have `patches-core` as a dependency and get it for free
in `#[cfg(test)]` contexts.

### Example

Before (Vca `multiplies_signal_by_cv`):

```rust
fn make_vca() -> Box<dyn Module> {
    let mut r = Registry::new();
    r.register::<Vca>();
    r.create("Vca", &AudioEnvironment { sample_rate: 44100.0, poly_voices: 16 },
        &ModuleShape { channels: 0, length: 0 }, &ParameterMap::new(), InstanceId::next()
    ).unwrap()
}

fn make_pool(n: usize) -> Vec<[CableValue; 2]> { vec![[CableValue::Mono(0.0); 2]; n] }

fn set_ports_for_test(module: &mut Box<dyn Module>) {
    // 0=in, 1=cv, 2=out
    let inputs = vec![
        InputPort::Mono(MonoInput { cable_idx: 0, scale: 1.0, connected: true }),
        InputPort::Mono(MonoInput { cable_idx: 1, scale: 1.0, connected: true }),
    ];
    let outputs = vec![OutputPort::Mono(MonoOutput { cable_idx: 2, connected: true })];
    module.set_ports(&inputs, &outputs);
}

#[test]
fn multiplies_signal_by_cv() {
    let mut m = make_vca();
    set_ports_for_test(&mut m);
    let mut pool = make_pool(3);
    pool[0][1] = CableValue::Mono(0.5);
    pool[1][1] = CableValue::Mono(0.8);
    m.process(&mut CablePool::new(&mut pool, 0));
    if let CableValue::Mono(v) = pool[2][0] {
        assert!((v - 0.4).abs() < f32::EPSILON);
    } else { panic!("expected Mono"); }
}
```

After:

```rust
#[test]
fn multiplies_signal_by_cv() {
    let mut h = ModuleHarness::build::<Vca>(&[]);
    h.set_mono("in", 0.5);
    h.set_mono("cv", 0.8);
    h.tick();
    assert_nearly!(0.4, h.read_mono("out"));
}
```

Multi-tick oscillator test, before:

```rust
let mut osc = make_osc_sr(frequency, sample_rate);
set_ports_outputs_only(&mut osc);
let mut pool = make_pool(8);
let mut first_cycle = Vec::with_capacity(period);
for i in 0..period {
    let wi = i % 2;
    osc.process(&mut CablePool::new(&mut pool, wi));
    if let CableValue::Mono(v) = pool[4][wi] { first_cycle.push(v); }
}
```

After:

```rust
let mut h = ModuleHarness::build::<Oscillator>(&params!["frequency" => frequency]);
let first_cycle = h.run_mono(period, "sine");
```

## Consequences

### Benefits

- **Test bodies express only intent.** Setup, teardown, and ping-pong management are
  invisible. A reader can understand what a test checks without knowing the pool layout.

- **Port index drift is impossible.** The harness derives cable indices from the descriptor;
  if `describe()` changes, the harness recalculates. Tests no longer need updating when a
  port is added or reordered.

- **Multi-tick tests have a standard shape.** `run_mono` / `run_mono_mapped` replace bespoke
  accumulation loops, making the pattern recognisable across modules.

- **Correct float comparison by default.** `assert_nearly` scales the tolerance correctly
  for any magnitude, eliminating the class of spurious failure (or spurious pass) caused by
  using raw `f32::EPSILON` for non-unit values.

### Costs

- **Harness hides the pool layout.** Tests that need to assert on internal pool state (e.g.
  verifying that a module does not write to an unconnected output cable) cannot use the
  harness directly. Such tests are rare and can use the existing lower-level API.

- **`test-support` feature must be kept out of release builds.** The harness uses
  `HashMap` and does runtime port lookups — neither is acceptable on the audio thread. The
  feature gate enforces this, but any accidental enabling in a release build would need to
  be caught by CI.

- **Migration touches every test file in `patches-modules`.** The migration is mechanical
  but not trivial; it should be done per-module in a dedicated ticket rather than all at once
  to keep diffs reviewable.

## Alternatives considered

### Per-module test helpers (status quo)

`make_vca`, `set_ports_for_test`, etc. solve the immediate repetition within a module but
do not generalise. Every new module author must write the same helpers from scratch. Rejected
as a long-term solution; the status quo is what motivates this ADR.

### Test helpers in `patches-integration-tests`

`HeadlessEngine` already provides a full-graph fixture. It is appropriate for testing module
interactions and plan lifecycle, but too heavyweight for single-module unit tests. Running
a full plan rebuild to test a VCA multiplies a value is unnecessary. `ModuleHarness` sits
at the unit test level; `HeadlessEngine` remains the integration test fixture.

### Macro-generated test modules

A macro could generate a complete Module stub (descriptor, set_ports, process) from a
compact DSL, replacing `PolyProbe`-style test modules in integration tests. This would solve
a related but distinct problem. Deferred: the harness addresses the higher-frequency unit
test pain first; test module generation is a follow-on if integration test boilerplate
continues to grow.
