# Polyphonic cables

Patches supports two cable kinds: **mono** (a single `f64` sample per tick) and
**poly** (an array of `f64` values, one per voice, per tick).

## Voice count

The voice count is fixed at engine initialisation via `AudioEnvironment::poly_voices`
(default: 16). All poly cables in a patch share this voice count.

## Poly modules

Poly-aware modules (e.g. `PolyOsc`, `PolyAdsr`, `PolyVca`) read and write poly
cables. Each voice is processed independently within the module's `process` call.

## Kind compatibility

Connecting a mono output to a poly input (or vice versa) is a validation error
caught by the interpreter. Use `MonoToPoly` or `PolyToMono` to bridge between
the two kinds explicitly.

## Cable slot initialisation

Newly allocated poly cable slots are zeroed with `Poly([0.0; 16])` rather than
`Mono(0.0)`. This prevents mono-initialised slots from being read as mono by
poly-aware modules during the first tick after a hot-reload.

## CableValue

```rust
pub enum CableValue {
    Mono(f64),
    Poly([f64; 16]),
}
```

`CableValue` derives `Copy`. The `CablePool` methods handle dispatching reads
and writes to the correct variant.
