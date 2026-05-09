# ADR 0071 — Multi-source input ports

**Date:** 2026-05-09
**Status:** Proposed
**Related:**
[ADR 0033 — Poly cables and layout strictness](0033-poly-cables.md),
[ADR 0047 — Trigger / mono-layout cables](0047-trigger-cables.md),
[ADR 0059 — Stereo cables and the mono→stereo broadcast](0059-stereo-cables.md),
ticket 0852 (synthesized-Sum fan-in rewrite — superseded by this ADR)

## Context

The current `InputPort` family (`MonoInput`, `PolyInput`, `StereoInput`) holds
exactly one cable index plus an affine map (`scale`, `offset`, `clip`) and,
for `StereoInput`, a `broadcast_from_mono` flag. `ModuleGraph::connect_with_map`
enforces this at build time: a second connection into the same input port
returns `GraphError::InputAlreadyConnected` (RT0001). Patches that fan multiple
sources into one input therefore needed an explicit `Sum` / `PolySum` /
`StereoSum` module to merge the signals.

Ticket 0852 added an automatic rewrite at descriptor bind: when the DSL
expresses fan-in, the interpreter synthesizes a Sum-family module of
`channels = N`, rewires the N source connections through it, and adds a
single sum→target edge. That keeps the user-visible DSL clean but at a cost:

- **Extra runtime overhead**: every multi-fan-in input pays for one extra
  module dispatch (`process()` frame), one extra cable hop, and one extra
  `read+write` pair per sample, on top of the unavoidable N reads + sum.
- **The graph lies to the user**: LSP graph view, SVG export, profiler
  per-instance CPU readouts, and docs all show `__autosum_*` synthetic
  nodes that don't appear in the source. The patch the user wrote and the
  graph the engine runs are no longer the same shape.
- **Split semantics for stereo broadcast**: mono→stereo broadcast lives on
  `StereoInput::broadcast_from_mono` for direct mono→stereo edges, but a
  group of mono sources fanning into a stereo target travels through a
  mono `Sum` followed by the broadcast — two different rewrite shapes for
  the same conceptual operation.
- **Three modules earning their keep on plumbing alone**: `Sum`, `PolySum`,
  and `StereoSum` exist almost entirely to express summation when
  multi-fan-in isn't allowed. They contribute no DSP that the input port
  itself couldn't perform.

The summation itself is unavoidable — multi-fan-in by definition requires
adding N sample streams. The question is *where* the addition happens.

## Decision

### 1. Input ports become multi-source

Each input port carries a fixed-capacity collection of `Source` records
instead of a single cable index plus map. The collection is allocated once
at graph-build time (when the cable builder populates ports) and never
resized on the audio thread.

```rust
pub struct Source {
    pub cable_idx: usize,
    pub scale: f32,
    pub offset: f32,
    pub clip: Option<(f32, f32)>,
    /// Stereo-only: mono producer broadcast as L = R = sample.
    pub broadcast_from_mono: bool,
}

pub struct MonoInput {
    pub sources: SmallVec<[Source; 1]>,
    pub connected: bool,
}

pub struct PolyInput   { /* same shape, Source has no broadcast flag */ }
pub struct StereoInput { /* same shape, Source.broadcast_from_mono in use */ }
```

`SmallVec<[Source; 1]>` keeps single-edge inputs (the common case) inline
with no heap allocation. Multi-edge inputs spill to a heap-allocated buffer
during build, never reallocated thereafter.

### 2. Read sums across all sources, applying each map separately

```rust
impl MonoInput {
    pub fn read(&self, pool: &[CableValue]) -> f32 {
        let mut acc = 0.0;
        for s in &self.sources {
            let raw = pool[s.cable_idx].as_mono() * s.scale + s.offset;
            let v = match s.clip {
                Some((lo, hi)) => raw.clamp(lo, hi),
                None => raw,
            };
            acc += v;
        }
        acc
    }
}
```

Per-source clip applies before summation — the same semantics today's
synthesized `Sum`-after-clip pipeline produces. Per-source `broadcast_from_mono`
on `StereoInput` lets stereo and mono producers coexist in one fan-in group:
each source contributes its `(L, R)` (broadcasting if it's mono) and the
sums add channel-wise.

### 3. Graph builder accepts multi-edge connections

`ModuleGraph::edges` keys on `(node, port, index)` and currently stores one
`Edge`. The storage becomes a `Vec<Edge>` per key; `connect_with_map`
appends rather than rejecting. The runtime invariant (no orphaned cable,
no orphaned consumer) is unchanged — `InputAlreadyConnected` retires.

Per-edge kind / poly-layout / mono-layout validation runs unchanged at
descriptor bind: every individual source must independently agree with
its target port. The previous "two sources are sometimes legal if you
inserted an explicit `Sum`" rule is gone — the input port itself is the
sum.

### 4. The synthesized-Sum rewrite (`fan_in.rs`) retires

`patches-interpreter::descriptor_bind::fan_in` and
`BindErrorCode::DuplicateInputConnection` go away. Bind passes the
multi-edge connection list straight to the builder.
`BindErrorCode::HeterogeneousFanIn` (introduced for fan-in into a kind-
mismatched target) becomes unreachable — every individual edge already
fails per-edge validation when its kind disagrees with the target.

### 5. `Sum` / `PolySum` / `StereoSum` retire

The three modules existed to express summation when input ports couldn't.
With multi-source ports, their behaviour is the input port's behaviour;
they earn no keep. Removed from `patches-modules`, the default registry,
and any docs / corpus / SVG. Existing patch fixtures that used `Sum`
explicitly are rewritten to fan straight into the consumer.

## Consequences

### Performance

- Single-source inputs (the common case) execute the same instructions as
  today: one `pool[idx]` read + scale + offset + clip.
- Multi-source inputs execute N reads + N affine applications + N adds,
  *one less* than today's `Sum`-rewrite (which adds an extra cable read +
  module dispatch frame).
- No allocation on the audio thread: the `SmallVec` is sized at build,
  never grows.
- Cache: a multi-source input's `Source` slice is contiguous; reads through
  it touch sequential cable indices in the order the cable builder packed
  them, preserving the cache-friendly layout desideratum (see CLAUDE.md
  §Design desiderata).

### API surface

- `MonoInput::cable_idx` (and the public field on `PolyInput` /
  `StereoInput`) is gone. Code that constructed `MonoInput { cable_idx, .. }`
  by hand — the harness, some tests, the cable builder — switches to
  `MonoInput::single(source)` / `MonoInput::from_sources(...)`.
- `MonoInput::scalar(idx, scale)` retained as a one-source convenience
  for tests and the cable builder fast path.
- `Module::set_ports` continues to receive `&[InputPort]`; only the inner
  shape of each variant changes.

### Patch behaviour

- Patches that used to fail with `BN0009` (duplicate input connection)
  now build, exactly as in ticket 0852. The implementation is different;
  the user-visible behaviour is the same minus the synthesized node.
- Patches that explicitly named a `Sum` / `PolySum` / `StereoSum` module
  no longer parse. Migration: replace `module mix : Sum(N)` plus N
  edges into `mix.in/i` plus `mix.out -> consumer.port` with N direct
  edges into `consumer.port`.

### What this is *not*

- Not a change to cable-pool storage. Cables, the cable allocator, and
  the per-thread partitioning desiderata are unaffected.
- Not a change to the `Module` trait or any module implementation; only
  the read-helper internals shift.
- Not a multi-output story. Outputs remain single-cable; only inputs
  carry the source collection.

## Phasing

The change is structural enough to want CI-green checkpoints. Three
tickets, each landable independently:

1. **Port shape** (ticket 0853). Replace the per-port single-source fields
   with `SmallVec<[Source; 1]>`. Reads iterate. Builder still emits at
   most one edge per input (length-1 in every port). No user-visible
   change.
2. **Builder accepts multi-edge** (ticket 0854). `ModuleGraph::edges`
   becomes `HashMap<key, Vec<Edge>>`; cable builder populates port
   `sources` from the edge list. Retire `fan_in.rs` and the
   `DuplicateInputConnection` bind error. Multi-fan-in patches now
   skip the synthesized-Sum rewrite.
3. **Retire Sum / PolySum / StereoSum** (ticket 0855). Delete the three
   module implementations, their registry entries, and any LSP corpus,
   SVG snapshot, or docs reference. Migrate any in-tree fixtures that
   used them to direct fan-in.

## Alternatives considered

**Status-quo synthesized Sum (ticket 0852 alone)**. Ships, fixes the
user-facing need. But the per-fan-in cost and the lying graph view are
real, and "ports do summation" is the shape the rest of the system
already implies (the cable pool already supports many readers per cable;
this just makes "many cables per reader" symmetric).

**Block-rate sum scratch buffer**. Stage all N source samples at the top
of the buffer, sum once. Wins nothing over per-sample summation when N
is small (the realistic case) and complicates the cable-pool invariant.
Pass.

**Keep `Sum` / `PolySum` / `StereoSum` as user-facing aliases**. Adds an
extra module dispatch for nothing once the input port already sums. If
a user wants a named summing node for clarity, it's a one-line template
in user code.

## References

- Ticket 0852 — `auto-sum-multi-fan-in.md` (the staging step; this ADR
  retires it).
- ADR 0033 §Poly layout strictness — per-edge layout validation is
  unchanged here; what changes is that *multiple* edges per input each
  carry their own (matching) layout independently.
- ADR 0059 §Stereo broadcast — `broadcast_from_mono` moves from a port-
  level flag to a per-`Source` flag, generalising cleanly to fan-in
  groups that mix mono and stereo producers.
