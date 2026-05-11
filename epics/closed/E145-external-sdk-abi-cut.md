---
id: E145
title: External SDK ABI cut — preparation
status: open
created: 2026-05-11
---

## Summary

Prepare the FFI surface (patches-ffi, patches-ffi-common) for being
extracted into a separate external module SDK repository. The C ABI
itself is in good shape — `#[repr(C)]` types only, plain function
pointers, single ABI version constant. The exposure is in three
adjacent areas:

1. **Backplane indices leak.** Plugins today receive raw scratch
   indices that include backplane slots (`AUDIO_OUT_L`,
   `GLOBAL_TRANSPORT`, host-control, tap, …). Any backplane reorg
   forces an ABI bump and plugin rebuild — already happened once in
   ABI v11 (ticket 0860).
2. **Descriptor JSON schema is implicit.** The wire schema lives in
   the hand-rolled deserializer in
   [patches-ffi-common/src/json/de.rs](patches-ffi-common/src/json/de.rs);
   no normative reference. External implementers (incl. a future C++
   SDK) need a spec.
3. **Packing rules are implicit.** ParamFrame scalar packing
   (sort by name+index, greedy-pack, align-up) and structural blob
   format only documented in code. Both sides recompute layout
   independently from the descriptor; if the algorithm ever changes,
   every external plugin breaks silently (caught by descriptor_hash,
   but only at load time).

Two structural changes (1) followed by two doc-only stabilisation
tickets (2, 3). After this epic the ABI surface is small enough,
spec'd enough, and decoupled enough from host internals that
extracting it to a separate repo no longer carries an ongoing risk
of ad-hoc bumps.

A C++ SDK against the same ABI becomes feasible at the close of this
epic (~600-1000 LOC + a JSON library). C++ SDK authoring is out of
scope here; the deliverable is the spec they would need.

## Tickets

- 0869 — Reorganise scratch low-end so sinks live above backplane.
  Today: `[sinks | backplane | dyn]`. After: `[backplane | sinks | dyn]`.
  Internal-only refactor; symbols (`MONO_READ_SINK`, `AUDIO_OUT_L`)
  unchanged, only their numeric values shift. ABI bump.
- 0870 — Pass plugin a scratch view that begins past the backplane.
  Loader passes `scratch_ptr.add(BACKPLANE_END)` and translates port
  cable indices into plugin-relative space. Planner forbids wiring
  any FFI plugin port to a backplane slot. After: backplane reorg
  no longer forces ABI bumps.
- 0871 — Write a normative descriptor JSON schema doc covering
  `ModuleDescriptor`, `ModuleDescriptorTemplate`, `ParameterKind` (all
  variants), `PortDescriptor`. Currently implicit in
  [patches-ffi-common/src/json/](patches-ffi-common/src/json/).
- 0872 — Document the wire packing rules: `ParamFrame` scalar area
  layout (sort + greedy align-pack), `PortFrame` layout
  (`PortFrameHeader` + arrays), structural blob format (already
  documented inline at structural_frame.rs:7-22 — copy out and
  formalise), `CableValue` (`[f32; 16]`, alignment 4) and cycle slot
  (`[CableValue; 2]`, ping-pong via `write_index`).

0869 → 0870 sequenced. 0871, 0872 independent of each other and of
the structural tickets.

## Out of scope

- Splitting patches-ffi / patches-ffi-common into a separate repo.
  This epic prepares; the cut is its own follow-on once the spec
  doc reviews come back clean.
- Adding a C builder API for descriptor emission (would let C++
  authors avoid pulling a JSON library). Discussed in the review
  but is an SDK ergonomics question, not a stabilisation question.
- C++ SDK implementation.
- ADR for the ABI cut. ADR 0072 covers the cable-pool layout
  decisions that 0869 extends; if 0869 gets contentious it can grow
  a phase-6 note rather than a new ADR. The spec docs from 0871/0872
  live under `docs/src/` (manual), not `adr/`.

## Notes

The descriptor_hash drift check
([patches-core/src/param_layout/hash.rs](patches-core/src/param_layout/hash.rs))
already protects against accidental schema or packing skew; it
detects the problem at load time and refuses the bundle. This epic
reduces *how often* that has to bite, not whether it bites.

ABI v11 is the current state ([patches-ffi-common/src/types.rs:17](patches-ffi-common/src/types.rs#L17)).
0869 + 0870 land together as v12. 0871/0872 are doc-only, no bump.
