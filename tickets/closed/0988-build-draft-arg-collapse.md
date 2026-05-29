---
id: "0988"
title: Pass PlanDecisions through build_draft (drop the 8-arg fan-out)
priority: medium
created: 2026-05-29
---

## Summary

`PatchBuilder::build_patch_with_meta` destructures `PlanDecisions { index,
topology, ports, buf_alloc, decisions }` and then re-broadcasts each part
into `build_draft` alongside `instance_ids`, `install_meta`, and `prev_state`
— eight arguments total, carrying `#[allow(clippy::too_many_arguments)]`.

This is the unpack-and-rebroadcast pattern ADR 0081 set out to remove:
`PlanDecisions` is a frozen bundle, and `build_draft` is exactly the consumer
that should read its fields from the bundle, not from eight loose parameters.

Two clean options:

1. **Pass `PlanDecisions` whole.** `build_draft` takes `PlanDecisions`,
   plus a small `ActionInputs { instance_ids, install_meta, prev_state }`
   bundle.
2. **Wrap into a single `DraftInputs`** that composes `PlanDecisions` with the
   action-phase inputs. Then `build_draft` takes one arg.

Either is acceptable; (1) is the smaller change and matches the
"compose frozen bundles" rule directly.

## Acceptance criteria

- [ ] `PatchBuilder::build_draft` takes at most three arguments (e.g.
      `PlanDecisions`, `&ActionInputs`, `&PlannerState`), or a single
      composing bundle.
- [ ] `#[allow(clippy::too_many_arguments)]` is removed from `build_draft`.
- [ ] `build_patch_with_meta` does not destructure `PlanDecisions` before
      passing it forward; it flows through as a value (the action phase may
      destructure once at the entry to its body).
- [ ] No new clone of any bundle field is introduced at the boundary;
      ownership flows decisions → build_draft → assemble as today.
- [ ] Audio goldens bit-identical; `just push` green.

## Notes

Part of epic **E162**. Depends on 0987 (cleaner edge type) and benefits from
0991 (per-input fused bundle removes one consumer of `topology.cable_fused`
inside `build_draft`). Pure signature / threading change — no logic touched.
Open question: whether the small `ActionInputs` bundle is worth its own type
or stays as three parameters. Default: three parameters; promote to a type
only if a downstream change needs to pass them as a unit.
