---
id: "0856"
title: Coordinate fusion (ADR 0072) and multi-source input ports (ADR 0071) — per-Source flag, slot tag, land order
priority: medium
created: 2026-05-09
epic: E141
adr: 0072
related: "E142, ADR 0071"
---

## Summary

ADR 0072 (cycle-free subgraph fusion) was written assuming the today
shape of `InputPort` — one cable per input. ADR 0071 (multi-source
input ports) replaces that with `SmallVec<[Source; 1]>` per port. The
two ADRs are independently sound but their implementations interact in
two specific places, plus a phasing constraint. This ticket captures
the coordination so the fusion tickets (0849, 0850) can be amended
when their turn comes, and the multi-source tickets (0853, 0854, 0855)
can land first to make the amendments straightforward.

## Coordination points

### 1. Phase 2 — `fused` flag is per-`Source`, not per-`InputPort`

Ticket 0849 ("Fusion phase 2 — fused reads in CablePool") currently
plans to add a `fused: bool` field to `InputPort` (per ADR 0072
§Engine change). Under ADR 0071, an input port carries N source
records and any individual source can sit on a fused or a delayed
cable. A VCA whose audio input fans in two branches — one inside an
acyclic chain, one closing a feedback loop with an LFO elsewhere —
needs per-source fused tracking.

The `Source` struct from ticket 0853 therefore gains a `fused: bool`
field. The read becomes:

```rust
for s in &input.sources {
    let slot = if s.fused { self.wi } else { 1 - self.wi };
    let raw  = pool[s.cable_idx + slot] * s.scale + s.offset;
    let v    = match s.clip { Some((lo, hi)) => raw.clamp(lo, hi), None => raw };
    acc += v;
}
```

The branch is predictable per source (a source's fusion status is
fixed by the plan and unchanged across ticks).

### 2. Phase 3 — slot tag is per-`Source`

Ticket 0850 ("Fusion phase 3 — two-region cable pool") plans to tag
each cable as `Slot::Scratch(idx)` or `Slot::Cycle(pair)`. Under ADR
0071 the tag belongs on each `Source`, replacing the `cable_idx:
usize` field. The read collapses to:

```rust
for s in &input.sources {
    let v = match s.slot {
        Slot::Scratch(idx) => pool_scratch[idx],
        Slot::Cycle(pair)  => pool_cycle[pair + (1 - self.wi)],
    };
    let raw = v * s.scale + s.offset;
    acc += match s.clip { Some((lo, hi)) => raw.clamp(lo, hi), None => raw };
}
```

The phase-2 `fused` flag becomes redundant once phase 3 lands — the
slot tag encodes both the location *and* the read semantics. Ticket
0850 should remove the bool when it migrates to tagged slots.

### 3. Land order — multi-source ports first

The cheap path is **E142 lands before 0849 (fusion phase 2 engine
ship)**:

- Ticket 0853 introduces `Source` with `cable_idx` + affine map +
  `broadcast_from_mono`. Per-port read iterates.
- Ticket 0854 lets the builder accept multi-edge inputs and retires
  the synthesised-Sum machinery from ticket 0852.
- Ticket 0855 deletes the `Sum` / `PolySum` / `StereoSum` modules.
- Then 0849 adds `fused: bool` to `Source` (one-line change), and
  the per-port flag never has to exist.

If 0849 lands first, it gets a per-port flag that tickets 0853 / 0854
have to refactor. Either order is correct; the **multi-source-first
order is one fewer migration step**. Ticket 0848 (fusion phase 1 —
planner-only SCC analysis, engine inert) is independent and can land
in any order.

## Acceptance criteria

- [ ] Ticket 0849 description updated to specify `fused: bool` on
      `Source` rather than on `InputPort`. Read example in the ADR
      / ticket body adjusted to the per-source loop shown above.
- [ ] Ticket 0850 description updated to specify `Slot` tag on
      `Source` rather than on the cable record, and to drop the
      now-redundant `fused: bool` from `Source` once `Slot` lands.
- [ ] ADR 0072 §Engine change gains a paragraph or footnote noting
      the dependency on ADR 0071 and the per-`Source` placement.
      ADR 0071 §"What this is *not*" gains a one-line cross-reference
      to ADR 0072 (the `fused` / `slot` knobs noted as future
      additions, not handled here). Rule of thumb: each ADR mentions
      the other once.
- [ ] CLAUDE.md desideratum "Parallelism-ready execution" has already
      relaxed under ADR 0072. No further amendment needed for ADR 0071
      (multi-source is read-side; ordering is unchanged). Confirm
      during review.
- [ ] No code changes in this ticket — it is pure coordination /
      doc-edit. Closes after the three description updates land.

## Notes

- The `Source` struct ends up carrying `cable_idx | slot` + `scale` +
  `offset` + `clip` + `fused` + (stereo-only) `broadcast_from_mono`.
  Two of those (`fused`, then later `slot`) come from the fusion ADR
  rather than the multi-source ADR. The split is OK as long as both
  ADRs document it; the alternative (one giant ADR covering both)
  fights the granularity that lets each phase ship independently.
- Floating-point summation order is fixed by the order the cable
  builder emits sources. Under either ADR alone the order is
  deterministic; under both together it stays deterministic. Snapshot
  tests that compare audio bit-exactly across plan rebuilds remain
  valid as long as the cable builder's emission order is stable
  (which is also the constraint ticket 0850's snapshot tests already
  rely on).
- LSP "this cable is in a feedback loop" inlay hints (ADR 0072 §Cycle
  detection) play with multi-source naturally: the hint attaches to
  the offending source, not the whole input port. One source on a
  back edge does not flag the others.
