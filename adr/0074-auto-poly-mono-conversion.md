# ADR 0074 — Auto poly↔mono conversion for Audio cables

## Status

Proposed (2026-05-15).

## Context

Poly cables are the third cable kind alongside mono and stereo. A poly
cable carries 16 lanes of `f32` discriminated by `PolyLayout` (`Audio`,
`Trigger`, `Transport`, `Midi`). Today, mono↔poly connections are
rejected in both the interpreter
([patches-interpreter/src/descriptor_bind/connections.rs]) and the core
graph ([patches-core/src/graphs/graph/mod.rs]).

The current rejection is friction for two common patterns:

1. **Mono→poly**: a single LFO or envelope driving all 16 voices. Today
   the author writes a `MonoToPoly` instance ([patches-modules/src/mono_to_poly.rs])
   between the two. The intent — "this single value applies to every
   voice" — is obvious; the manual wiring is busywork.

2. **Poly→mono**: poly-voice audio reaching the patch's mix bus. Every
   poly synth ends with this fold. `PolyToMono`
   ([patches-modules/src/poly_to_mono.rs]) already exists and does
   exactly one thing: sum all 16 lanes.

The cable protocol does not separate audio from CV (both are `f32`
samples), so we cannot gate sugar on "is this a pitch CV?" by structural
means. Users who route control-rate signals through an auto-fold will
hear obvious wrongness immediately; the dominant case is poly audio
voices summing to a mono bus.

## Decision

Extend the existing multi-source fan-in auto-Sum pattern (see ADR 0071's
rejection rationale and [patches-interpreter/src/descriptor_bind/fan_in.rs])
to cover mono↔poly Audio kind mismatches.

### Mechanism: synthetic-module insertion in descriptor_bind

A new pass in `patches-interpreter::descriptor_bind`, sibling to
`coalesce_fan_in`, walks the bound connections and detects edges where
the source and target cable kinds differ in one of the two
sugar-accepted combinations:

| Source kind          | Target kind          | Synthetic module |
| -------------------- | -------------------- | ---------------- |
| `MonoLayout::Audio`  | `PolyLayout::Audio`  | `MonoToPoly`     |
| `PolyLayout::Audio`  | `MonoLayout::Audio`  | `PolyToMono`     |
| `PolyLayout::Audio`  | `CableKind::Stereo`  | `PolyToMono`     |

The third row composes this pass with the existing mono→stereo
broadcast coercion (ADR 0059 §2): a poly Audio source feeding a stereo
input has a `PolyToMono` inserted here, leaving a Mono Audio → Stereo
edge that the runtime ModuleGraph broadcasts at construction time.
This case has no separate "PolyToStereo" module — the two existing
sugar paths just compose. Poly Audio summing into a stereo bus
(`PolyToMono → broadcast`) is what every poly patch routing voices
into `FdnReverb` / `StereoLimiter` wants; making the user spell that
out is busywork for the same reasons that motivate the mono↔poly
sugar.

For each such edge, the pass:

1. Synthesizes an instance of the appropriate module with a naming
   prefix `__autoconv_` (mirroring `__autosum_` for fan-in auto-Sum).
2. Rewrites the original connection: source → synthetic, synthetic →
   original target.
3. Records the synthetic instance in the bound module list so the
   runtime ModuleGraph builder treats it as a normal node.

All other kind combinations (Trigger, Transport, MIDI on either side;
mono↔stereo, which is already handled by the existing
`broadcast_from_mono` port flag) continue to use their current paths
and current rejection rules.

### No runtime cost via ADR 0072 fusion

The synthetic module sits between two SCCs in the typical cycle-free
case. ADR 0072 cycle-free subgraph fusion already eliminates the
1-sample cable delay across SCC boundaries: producer writes the current
tick, synthetic reads the current tick (fused), synthetic writes the
current tick, consumer reads the current tick (fused). The synthetic
hop adds zero tick latency and one fused-read at runtime.

This is the same logic ADR 0071 used to reject multi-source input
ports in favour of `coalesce_fan_in`'s auto-Sum: fusion makes the
synthetic-node hop free, so the structural cost of port-signature
changes (or, in this ADR's earlier draft, port-read flags crossing the
FFI ABI) is not worth paying.

### Non-Audio layouts stay rejected

Mono↔poly conversions are restricted to `MonoLayout::Audio ↔
PolyLayout::Audio` (and, by composition with broadcast,
`PolyLayout::Audio → CableKind::Stereo`). Any other combination raises
`CableKindMismatch` unchanged. Summing 16 trigger streams or
broadcasting one trigger to 16 voices is semantically loaded in a way
audio summation is not — users wanting that behaviour write an
explicit module.

Stereo → Poly Audio is *not* covered. Splitting a stereo cable into
poly voices has no single obvious behaviour (broadcast both channels?
fan L to even lanes, R to odd? sum first?), and the use case is
absent: poly synthesis flows poly→mono/stereo at the mix bus, not the
other way. Users wanting stereo→poly write an explicit
`StereoSplitter` followed by `MonoToPoly`.

### Surface tools filter the prefix

A new `QName::is_synthetic()` umbrella helper covers both
`__autosum_` and `__autoconv_`
([patches-core/src/qname.rs:15](../patches-core/src/qname.rs#L15)).
Existing filter sites in SVG export
([patches-svg/src/flat_to_layout.rs](../patches-svg/src/flat_to_layout.rs)),
profiler timing collection
([patches-profiling/src/timing_collector.rs](../patches-profiling/src/timing_collector.rs)),
and LSP surface providers sweep onto `is_synthetic()` so
`__autoconv_*` instances are hidden everywhere `__autosum_*` already is.

## Alternatives considered

**Port-read flag (mirror stereo broadcast).** Add `broadcast_from_mono`
to poly inputs and `sum_from_poly` to mono inputs; planner sets them at
build time; pool checks them at read time. Cheaper at runtime (one
branch vs one module hop) but:

- Crosses the FFI ABI (plugins reconstruct port structs from the wire
  frame), requiring an ABI version bump for the `sum_from_poly` byte.
- Diverges from the established auto-Sum precedent.
- Hides the conversion in port metadata instead of representing it in
  the graph.

Rejected: fusion eliminates the runtime cost of the synthetic-module
approach, removing the only real advantage of the flag approach. The
ABI invariance and pattern consistency dominate.

**Synthetic node inserted at planner / cable-builder time.** Same
effect as the descriptor_bind approach but later in the pipeline.
Rejected: ADR 0071 / `coalesce_fan_in` set the precedent at
descriptor_bind; placing this pass alongside it keeps the
graph-rewriting logic in one stage, before the runtime ModuleGraph is
constructed.

**Reject poly→mono, require explicit fold.** Considered. Rejected
because sum is the standard poly synth output fold and the
asymmetric-destructiveness argument relative to mono→stereo broadcast
doesn't survive the symmetry observation: broadcast is "wrong" for CV
in the same way sum is "wrong" for CV, and we shipped broadcast.

**Average instead of sum.** Rejected: average is almost never the right
operation. Voice levels are set assuming sum at the output; averaging
silently halves perceived loudness as voice count grows. Sum matches
`PolyToMono`'s existing behaviour.

## Consequences

- One new pass in `patches-interpreter::descriptor_bind`, sibling to
  `coalesce_fan_in`. Reuses the existing module-synthesis helper if one
  is factored out of `fan_in.rs`; otherwise mirrors its shape directly.
- Two existing module types (`MonoToPoly`, `PolyToMono`) gain
  programmatic use; their direct user-facing utility is reduced but
  they remain for explicit cases.
- The runtime ModuleGraph, planner, and `ExecutionPlan` are unchanged —
  synthetic instances appear as ordinary nodes.
- **No FFI ABI impact.** Synthetic modules are host-side instances; FFI
  plugins are unaffected.
- LSP / SVG / profiler tooling filters `__autoconv_` alongside
  `__autosum_` via a new `QName::is_synthetic()` umbrella helper;
  existing filter sites sweep onto it.
- Audio-integrity golden corpus gains two patches exercising the new
  conversions; existing goldens are unaffected.
- Project docs (`CLAUDE.md`, manual) deliberately not updated as part of
  this work — docs are lapsing pending a thorough rewrite; this ADR is
  the durable record of the behaviour until then.

## See also

- [ADR 0059 — Stereo cables and tap unification](0059-stereo-cables-and-tap-unification.md)
- [ADR 0071 — Multi-source input ports (Rejected)](0071-multi-source-input-ports.md)
  — explains why fusion makes synthetic-module insertion the right
  shape for sugar like this.
- [ADR 0072 — Cycle-free subgraph fusion](0072-cycle-free-subgraph-fusion.md)
  — provides the zero-delay guarantee that justifies this approach.
- [Epic E147 — Auto poly/mono cable conversion](../epics/open/E147-auto-poly-mono-conversion.md)
