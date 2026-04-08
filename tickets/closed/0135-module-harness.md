---
id: "0135"
title: Add ModuleHarness to patches-core (test-support feature)
priority: high
created: 2026-03-18
---

## Summary

Add `ModuleHarness` to `patches-core` as specified in ADR 0018. The harness owns a
module instance and its cable pool, derives port-to-cable assignments from the descriptor,
and exposes a named-port interface — eliminating pool sizing, `set_ports` setup,
ping-pong management, and `CableValue` unwrapping from test bodies.

## Acceptance criteria

- [ ] `patches-core/Cargo.toml` has a `test-support` feature. The feature is not enabled
      by default. The `test-support` module is compiled when `cfg(any(test, feature = "test-support"))`.
- [ ] `ModuleHarness` is in `patches-core/src/test_support/harness.rs` and re-exported
      from `patches_core::test_support`.
- [ ] `ModuleHarness::build::<M>(params: &[(&str, ParameterValue)]) -> Self`:
      - Registers and creates the module via `Registry`.
      - Uses `AudioEnvironment { sample_rate: 44100.0, poly_voices: 16 }` and
        `ModuleShape { channels: 0, length: 0 }` as defaults.
      - Reads the descriptor to derive a cable index for each input and output port,
        in descriptor order.
      - Allocates a pool sized to `(inputs.len() + outputs.len()) * 2` `CableValue` slots.
      - Calls `set_ports` with all ports connected and scales of `1.0`.
- [ ] `set_mono(name, value)` writes a `CableValue::Mono(value)` to the read slot for
      the input named `(name, 0)`.
- [ ] `set_mono_at(name, index, value)` writes to the input named `(name, index)`.
- [ ] `tick()` calls `module.process(&mut CablePool::new(&mut pool, wi))`, then advances
      `wi` to `1 - wi`. Returns `&mut Self` for chaining.
- [ ] `read_mono(name) -> f32` reads the write slot for the output named `(name, 0)` and
      panics with a clear message if the value is not `CableValue::Mono`.
- [ ] `read_mono_at(name, index) -> f32` reads by `(name, index)`.
- [ ] `run_mono(ticks, output) -> Vec<f32>` runs `ticks` ticks and collects the named output.
- [ ] `run_mono_mapped(ticks, input, inputs, output) -> Vec<f32>` feeds `inputs` (cycled
      if shorter than `ticks`) into the named input each tick and collects the named output.
- [ ] Poly equivalents: `set_poly`, `set_poly_at`, `read_poly`, `read_poly_at`,
      `run_poly`, `run_poly_mapped` (operating on `[f32; 16]` and `CableValue::Poly`).
- [ ] Panics with a descriptive message if a port name is not found in the descriptor.
- [ ] Unit tests in `patches-core` cover the harness using a minimal test-only module
      (or an existing simple module if accessible): correct read/write round-trip,
      correct tick advancement, `run_mono` collects the right number of samples.
- [ ] `cargo build`, `cargo test`, `cargo clippy` pass with no warnings.

## Notes

See ADR 0018 for the full specification and before/after examples.

The harness uses `HashMap` for port lookups and is not suitable for the audio thread.
The `test-support` feature gate enforces this.

For the default `AudioEnvironment` and `ModuleShape`, consider whether `build_with_env`
and `build_with_shape` variants are needed. Defer unless a test in T-0137 requires them.
