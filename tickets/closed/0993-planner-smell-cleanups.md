---
id: "0993"
title: Planner smell cleanups — pack_frame helper, dead fields, inline stage helpers
priority: low
created: 2026-05-29
---

## Summary

Loose-ends bucket from the E160 post-landing review. Each item is small;
grouping them reduces the per-PR overhead.

1. **`pack_frame` helper.** `pack_into` is called with the same shape twice:
   in `instantiate` for install (line 878) and in `build_draft` for surviving
   updates with non-empty `param_diff` (line 699). Both do
   `ParamFrame::with_layout` → `defaults_from_descriptor` → `pack_into` →
   wrap error into `BuildErrorKind::{ModuleCreationError,InternalError}`.
   Extract a `pack_frame(desc, layout, params) -> Result<ParamFrame, BuildError>`
   helper used at both sites.

2. **Dead field: `ResolvedGraph.index`.** Held as `&'a GraphIndex<'a>` with
   `#[allow(dead_code)]`. Drop it (and the `#[allow]`) if no consumer reads
   it; lifetime becomes simpler.

3. **Dead field: `BufferAllocState::scratch_hwm`.** Doc-commented as
   "diagnostic-only, not consulted by the next allocation pass." Audit
   consumers: if only tests / diagnostics read it, leave with a
   `#[cfg(...)]` or move to a sibling diagnostic struct; if nothing reads
   it, remove. Resolve together with 0992 if both are in flight.

4. **Inline single-use `impl::build` helpers.** Three free functions in
   `state/mod.rs` are used by exactly one stage's `build` site each, with no
   other read site:
   - `resolve_output_port_positions` → `PortClassification::build`
   - `classify_producer_ports` → `PortClassification::build`
   - `compute_order_with_fusion` → `Topology::build`
   Move each into the corresponding `impl::build` body (or an associated
   `fn`). Reduces module-level surface.

5. **`build_input_buffer_map` error context.** Currently emits
   `PlanError::Internal(format!(...))` for three different missing-key
   conditions (node, output port, buffer). Adding three structured variants
   would be churn; keeping the strings is fine, but document the contract
   (single-derivation-site for output_buf keys already implies most of these
   are unreachable).

## Acceptance criteria

- [ ] `pack_frame` helper exists; both call sites use it. Error wrappers at
      call sites map to the appropriate `BuildErrorKind` variant (install →
      `ModuleCreationError`, update → `InternalError`, preserving today's
      semantics).
- [ ] `ResolvedGraph.index` field is dropped (or genuinely consumed by a
      reader). No `#[allow(dead_code)]` remains on it.
- [ ] `BufferAllocState::scratch_hwm` audit: dropped if unread, or doc
      updated with the specific reader(s) and gated under `#[cfg(test)]` /
      moved to a diagnostic struct if read only there.
- [ ] `resolve_output_port_positions`, `classify_producer_ports`, and
      `compute_order_with_fusion` are moved into their consuming
      `impl::build` bodies (or assoc fns) — no remaining `pub(crate)`
      module-level surface for them.
- [ ] No clippy / rustc warnings introduced. `just push` green.
- [ ] Audio goldens bit-identical.

## Notes

Part of epic **E162**. Independent of every other ticket — purely local
cleanups. Low-priority bucket; can be landed alongside other epic work or
left as a polishing pass at the end. Each item is independently revertable;
ship as one PR or split per-item — reviewer's choice.
