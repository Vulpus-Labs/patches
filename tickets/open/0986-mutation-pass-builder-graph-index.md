---
id: "0986"
title: Mutation testing pass — builder + graph_index
priority: medium
created: 2026-05-29
---

## Summary

Extend the mutation-testing pass to `patches-planner/src/builder/mod.rs` and
`patches-planner/src/state/graph_index.rs`, completing the planner-crate
audit. Triage every surviving mutation by the same workflow as 0985.

## Acceptance criteria

- [ ] `cargo-mutants` scope extended to include `builder/mod.rs` and
      `graph_index.rs`.
- [ ] Triage of every reported `MISSED` mutation: covered by a new test (unit
      or property) or annotated `#[mutants::skip]` with a one-line
      `// MUTANTS: <reason>` comment.
- [ ] Triage table recorded in ticket close notes.
- [ ] Second full run on the extended scope reports zero `MISSED`.
- [ ] Combined `cargo-mutants` pass over `patches-planner/src/{state, builder}`
      completes within the wall-clock budget documented in 0984 and cleans up
      its workspace.
- [ ] `just push` + `just smoke` green.

## Notes

Part of epic **E161**, phase P4. Depends on 0983 (history-replay properties in
place) and 0985 (initial pass triage complete).

Expected high-value catches:

- `build_input_ports` `unwrap_or(&true)` for the fused fallback — flipping to
  `&false` regresses disconnected ports (which are fused by definition).
- `build_output_ports` `to_zero_set.contains(...)` guard — removing the
  conditional causes over-emission into `to_zero_poly`.
- `classify_nodes`' AND-of-three survivor guard (`module_name && shape &&
  structural`) — dropping a conjunct silently mis-survives a rebuild. The
  three existing guard tests in `state/tests.rs` should bite individually;
  the mutation pass reveals whether they bite when guards interact.
- `ModuleAllocState::diff` tombstone selector (`if !new_ids.contains(&id)`) —
  dropping the `!` would tombstone survivors instead of removals.
- The builder's conditional push sites: `installs.push`,
  `parameter_updates.push`, `param_frames.push`, `port_updates.push`,
  `periodic_indices.push`, `to_zero_poly.push`. Skipping any push or
  duplicating it changes plan content silently.
- `instantiate` minting via `id_source.next_id()` — bypassing the source
  (e.g. replacing with `InstanceId::next()`) defeats the 0978 injection
  contract. Should be caught by `instantiate_uses_injected_id_source` if the
  assertion is rigorous.

After this ticket, every load-bearing site in the planner crate has either an
explicit covering test or a documented equivalence annotation. Any future
mutation-testing regression on the configured scope is then a real signal that
new code is under-tested.
