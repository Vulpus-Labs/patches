---
id: "0986"
title: Mutation testing pass — builder + graph_index
priority: medium
created: 2026-05-29
closed: 2026-05-29
---

## Done

Scope: `patches-planner/src/builder/mod.rs` + `patches-planner/src/state/graph_index.rs`.
Initial pass: **61 mutants, 9 missed**. After triage: **0 missed** (45
mutants — the `check_ffi_port_offsets` `#[mutants::skip]` annotation
removed 16 boundary mutations from the pool).

Combined pass over `patches-planner/src/{state, builder}/`: **118
mutants, 0 missed** (78 caught, 40 unviable) in **89 s** wall-clock —
well within the 0984 budget. The full planner audit landed.

## Triage table

| Mutation                                                                            | Outcome | Action                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| ----------------------------------------------------------------------------------- | ------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `builder/mod.rs:380:16` `<` → `<=` in `check_ffi_port_offsets` (input loop)         | skipped | `#[cfg_attr(test, mutants::skip)]` on `check_ffi_port_offsets`. First conjunct `idx < SCRATCH_CAPACITY` masks `< → <=`: differs only at `idx == 2048`, where the second conjunct requires `2048 < scratch_base_offset` but `scratch_base_offset ≤ RESERVED_SLOTS = 32`. Observably equivalent under any real input.                                                                                                                                                                                                                                                                                    |
| `builder/mod.rs:395:16` `<` → `<=` in `check_ffi_port_offsets` (output loop)        | skipped | Same reason; same `mutants::skip`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `builder/mod.rs:395:64` `<` → `<=` (output's second `<`, the live check)            | covered | New `ffi_output_at_backplane_boundary_is_accepted` in `backplane_bind.rs`: asserts an FFI output at `cable_idx == BACKPLANE_SIZE` is accepted. Killed locally even though `mutants::skip` on the function now hides it from cargo-mutants.                                                                                                                                                                                                                                                                                                                                                             |
| `builder/mod.rs:602:34` `<=` → `>` on `m.names.len() <= pool_index`                 | covered | New `monitor_meta_names_and_types_indexed_by_pool_slot`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `builder/mod.rs:603:47` `+` → `*` and `+` → `-` on `m.names.resize(pool_index + 1)` | covered | Same test — asserts every active slot has a populated `Some(name)` and non-empty type at its pool index.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `builder/mod.rs:604:47` `+` → `*` and `+` → `-` on `m.types.resize(pool_index + 1)` | covered | Same test.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| `builder/mod.rs:667:61` `!=` → `==` on `prev_ns.input_ports != input_ports`         | covered | New `late_input_connection_on_sink_emits_port_updates`. The existing `late_connection_emits_port_updates_for_both_endpoints` happened not to bite this mutation reliably (process-local pool-slot assignment interacted with the assertion shape — see `project_pool_slot_nondeterminism`). The new test uses a `TrackerSink` with no outputs, so the consumer's `output_ports` vector is identically empty in both plans; the input-side `!=` is then the **only** conjunct that can fire, and dropping or flipping it leaves `ports_changed = false` and the port_updates entry is silently dropped. |

## Wrapper note

`tools/run-mutants.sh` already had the `-- --test properties` selector
removed during 0985's triage; nothing to change here. The combined
pass times confirm worker mode + `--jobs 4` is the right default.

## Verification

```sh
$ just mutants \
    --file patches-planner/src/state/alloc.rs \
    --file patches-planner/src/state/mod.rs \
    --file patches-planner/src/builder/mod.rs \
    --file patches-planner/src/state/graph_index.rs
118 mutants tested in 89s: 78 caught, 40 unviable
[run-mutants] removing mutants.out/ (exit 0)
```

`just push` green (test 60 s / clippy 8.5 s). `just smoke` green
(integration 188 / 188).

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
