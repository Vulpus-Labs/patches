# Cable pool internals

## The flat buffer pool

All cable values live in a single contiguous allocation: a `Vec<[CableValue; 2]>`.
Each element is one cable, and the two-element inner array is its ping-pong pair.

```
pool[cable_idx][0]   ←→   pool[cable_idx][1]
```

`CableValue` is a `Copy` enum:

```rust
pub enum CableValue {
    Mono(f32),
    Poly([f32; 16]),
}
```

## Ping-pong and the 1-sample delay

Every tick, `wi` (write index) alternates between `0` and `1`.

- **Write slot** (`wi`): the module writes its output here during the current tick.
- **Read slot** (`1 - wi`): what other modules read — what was written *last* tick.

This 1-sample delay is intentional and load-bearing. Because every module reads
last tick's values and writes to the current tick's slot, modules can be scheduled
in any order without data races or order-dependent results. Feedback connections
(where a module's output is wired back to its own or an upstream module's input)
are also well-defined — they simply see the previous tick's value.

`CablePool<'a>` wraps the pool and the current `wi`:

```rust
pub struct CablePool<'a> {
    pool: &'a mut [[CableValue; 2]],
    wi: usize,
}
```

The `'a` lifetime ties each `CablePool` to one exclusive access window per tick.
This prevents a second `CablePool` (or any other mutable reference into the same
buffer) from being alive simultaneously, enforcing the single-writer guarantee at
compile time.

## Reserved slots

The first 16 slots of every pool are reserved for infrastructure. No dynamically
allocated cable ever occupies these indices.

| Index | Constant | Purpose |
|---|---|---|
| 0 | `MONO_READ_SINK` | Permanent mono zero; disconnected `MonoInput` ports point here |
| 1 | `POLY_READ_SINK` | Permanent poly zero; disconnected `PolyInput` ports point here |
| 2 | `MONO_WRITE_SINK` | Mono write drain; unconnected `MonoOutput` ports point here |
| 3 | `POLY_WRITE_SINK` | Poly write drain; unconnected `PolyOutput` ports point here |
| 4 | `AUDIO_OUT_L` | Left audio output; `AudioOut` writes here; audio callback reads here |
| 5 | `AUDIO_OUT_R` | Right audio output |
| 6 | `AUDIO_IN_L` | Left audio input (reserved, not yet used) |
| 7 | `AUDIO_IN_R` | Right audio input (reserved, not yet used) |
| 8 | `GLOBAL_CLOCK` | Absolute sample counter, written by audio callback each tick |
| 9 | `GLOBAL_DRIFT` | Slowly varying value in `[-1, 1]` for globally correlated pitch drift |
| 10–15 | — | Reserved for future backplane use |

## Disconnected ports and null slots

When a port has no cable attached, the planner assigns it to a sink/source slot
rather than a real cable slot:

- A disconnected `MonoInput` gets `cable_idx = MONO_READ_SINK`. Reading it always
  yields `0.0 × scale = 0.0`. The `connected` field is `false`.
- A disconnected `MonoOutput` gets `cable_idx = MONO_WRITE_SINK`. Writes go there
  harmlessly; nothing reads from it.
- Same pattern applies for `PolyInput` / `PolyOutput` with their respective sinks.

This means `process` never needs to branch on whether a port is connected in order
to avoid an out-of-bounds read — it can always call `pool.read_mono(&self.port)`
safely. The `connected` field exists for modules that want to *skip work* on
disconnected outputs (e.g. an oscillator might skip computing the triangle wave if
`out_triangle.is_connected()` is false).

## Global backplane

The audio callback writes directly to backplane slots before calling `tick()` each
sample:

- `GLOBAL_CLOCK` receives the absolute sample counter as `CableValue::Mono(count)`.
- `GLOBAL_DRIFT` receives a slowly varying random-walk value, shared across all
  oscillators that opt into drift.

The oscillator reads `GLOBAL_DRIFT` via a fixed `MonoInput` whose `cable_idx` is
hardcoded to `GLOBAL_DRIFT` at construction time — no DSL cable connection is
needed or possible for backplane ports.
