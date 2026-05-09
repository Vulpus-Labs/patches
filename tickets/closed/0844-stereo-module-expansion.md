---
id: "0844"
title: Desugar — stereo module to splitter / paired mono / joiner
priority: medium
created: 2026-05-08
closed: 2026-05-09
epic: E140
adr: 0070
depends-on: "0843"
---

## Summary

Implement the expansion algorithm in ADR 0070 §"Expansion algorithm"
inside the DSL pipeline. Given the AST produced by the pest grammar
extension (ticket 0843), produce a flat patch indistinguishable from
the hand-written splitter / paired-mono / joiner form.

The stage runs after parse and before existing scope/expand resolution
in `patches-dsl/src/expand/`. The output is the same `FlatModule` /
`FlatConnection` set today's expander produces, so downstream stages
(validate, planner, runtime) need no changes.

After the 2026-05-09 redesign, side-specific params are recognised by
inspecting the existing `param_block`: `@l` / `@r` at_blocks become
per-channel overrides, and the equivalent `key[l]` / `key[r]` indexed
forms are normalised into the same. Channel selectors on ports are
recognised by inspecting `PortIndex::Name { name: "l" | "r", ... }`
and matching the addressed module against its `is_stereo` flag.

## Acceptance criteria

- [x] `stereo module X : T` decl rewritten to two mono decls
      `X__l : T`, `X__r : T` with shared params plus per-channel
      overrides merged in `stereo_desugar::split_params`. Conflict
      resolution: override wins.
- [x] Per-key indexed overrides (`rate[l]: 0.8`) accepted equivalently
      to `@l: { rate: 0.8 }`; both reduce to the same merged side
      params.
- [x] **Single-channel constraint (heuristic).** Reject
      `stereo module x : T(channels: N)` for `N > 1` at desugar time
      with `ST0043 StereoMultiChannelType`. Full descriptor-driven
      check (catching default-multi-channel types) is a follow-up
      ticket; the desugar pass has no descriptor registry access, so
      the heuristic only sees what the AST carries.
- [x] Identifier-clash check: a user module named `X__l` / `X__r` that
      collides with a synthesised name produces `ST0041`
      `StereoIdentClash` at the colliding module's decl site.
- [x] Cable rewriting per ADR 0070 §"Connection rules":
  - mono → bus: edge duplicated to both sides
  - known-stereo external → bus: splitter inserted with CSE
  - stereo module → bus on stereo module: pair-direct (no splitter)
  - mono → `port[l]` / `port[r]` selector: direct edge to that side
  - stereo source → `port[l]` / `port[r]`: `ST0042`
    `StereoBusToSide` error
- [x] `port[l]` / `port[r]` against a non-stereo module fall through
      to existing alias / param-arity resolution unchanged.
- [x] Splitter CSE: same `(module, port)` consumed by multiple stereo
      modules emits exactly one `StereoSplitter`.
- [x] Joiner emission: a stereo module's bus output emits a
      `StereoJoiner` only when at least one consumer reads
      `name.<port>` (bus form, no `[l]` / `[r]`). Side-tap-only
      consumption emits no joiner.
- [x] Mixed bus + side-tap consumption emits one joiner whose output
      fans to bus consumers; side-tap consumers read the underlying
      mono instance directly.
- [x] Per-channel override params are merged onto the relevant side
      only; shared params apply to both. Override wins for duplicate
      keys.
- [ ] Diagnostics for stereo→mono and stereo→side-tap include the
      escape hatches (`Sum`, pick a side via `port[l]` / `port[r]`)
      as fix-it *suggestions in the message body*. The structured
      fix-it carrying the suggested replacement spans is a follow-up
      LSP ticket.
- [x] Drums example from ADR 0070 §"Worked example" desugars to a
      flat patch with the expected topology — splitter at `mix.out`,
      joiner at `out_crush.out`, paired Bitcrushers, broadcast LFO.
      `tests/expand/stereo.rs::drum_worked_example_topology`. Names
      differ from the hand-written `out_crush_l` / `_r` form (the
      desugar uses `__l` / `__r` per ADR), so the comparison is
      topological, not byte-for-byte.
- [x] Hot-reload re-runs desugar; instance state is preserved per
      `__l` / `__r` instance via the existing `InstanceId` registry
      (ADR 0003). No new state-preservation work needed — verified by
      existing hot-reload integration tests.

### Out of scope (follow-up tickets)

- **Stereo decls inside template bodies.** Currently rejected at
  desugar with `ST0040` `StereoInTemplate`. Supporting them cleanly
  needs the descriptor registry that the template inliner has
  access to — better implemented after `CableKind::Stereo`
  propagation.
- **Descriptor-driven single-channel check.** The current heuristic
  only catches explicit `channels: N > 1` shape args. The full check
  (which would also catch types whose descriptor declares default
  channels > 1, or per-axis port multiplicities) is a binding-stage
  task in `patches-interpreter`.
- **Structured fix-it spans on stereo→mono / stereo→side errors.**
  The error messages name the escape hatches inline; the LSP-side
  fix-it surface lands in 0846.

## Implementation notes

Landed as `patches-dsl/src/stereo_desugar.rs`, called from
`expand::expand` before tap and host-control desugars (so they see a
homogeneous mono module surface).

**Always-insert** strategy avoids any descriptor lookup. A
`StereoSplitter` is inserted at every stereo-module bus input
regardless of source kind: the planner's existing
`CableKind::Mono → CableKind::Stereo` broadcast rule promotes a mono
source feeding the splitter's stereo input transparently, so the
desugar produces identical-to-hand-written output without knowing
whether the source is mono or stereo. Symmetrically, a
`StereoJoiner` is inserted at every stereo-module bus output (when at
least one consumer reads bus form). The pair-direct optimisation
elides both when both endpoints are stereo modules in bus form
(purely name-based).

Splitter / joiner CSE state: `BTreeMap<(String, String), String>`
keyed on `(source_module_name, source_port)`. Synthesised names use
the reserved `~` prefix (`~split_<n>` / `~join_<n>`) which the lexer
rejects in user identifiers, guaranteeing collision-free instance
names.

Cable scale lives on per-consumer cables (`~split.out_{l,r} → target`
and `source → ~join.out`-style consumers), not on the synthesised
feed cables — so multiple consumers sharing a splitter / joiner via
CSE each carry their own scale.

Param merge: `split_params` walks the param block, partitioning into
shared / l-overrides / r-overrides. Override entries shadow shared
keys with the same name; preserves source order otherwise.

## Test surface

`patches-dsl/tests/expand/stereo.rs` — 20 tests covering decl rewrite,
param merge (`@l` / `@r` and `key[l]` / `key[r]` forms), the
override-wins semantics, side-selector rewrites, mono-broadcast,
splitter CSE, joiner emission, side-tap-only joiner suppression,
mixed bus + side-tap, identifier clash, stereo-bus-to-side error,
template rejection, multi-channel-type rejection, and the ADR §"Worked
example" topology.
