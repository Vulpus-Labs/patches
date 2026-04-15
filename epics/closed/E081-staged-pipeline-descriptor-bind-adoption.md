---
id: "E081"
title: Staged pipeline — descriptor-bind adoption & polish
created: 2026-04-15
status: closed
depends_on: ["E080", "ADR-0038"]
tickets: ["0435", "0436", "0437", "0438"]
---

## Summary

Follow-up work left deferred after 0432 landed the LSP staged pipeline
and `descriptor_bind`. The `BoundPatch` artifact is produced and cached
but not yet consumed by feature handlers; pipeline diagnostics are
collapsed onto the root URI; pipeline-layering (`PV####`) warnings
have the rendering path but no emission sites; `patches-interpreter::build`
still re-resolves descriptors rather than starting from a bound graph.

Three of the four tickets are independent of the tree-sitter gating
work (ticket 0433); one — diagnostic bucketing — is naturally paired
with it, because once TS stops running on clean-pest files the
pipeline's cross-file diagnostics must land on the correct URI or
included files lose coverage.

## Acceptance criteria

- [ ] LSP feature handlers resolve module descriptors and port kinds
      through the cached `BoundPatch` rather than calling `Registry`
      directly or walking raw `FlatPatch` fields.
- [ ] Pipeline diagnostics bucket by source URI and publish per-URI;
      no more "in <path>:" message prefixes on the root URI's list.
- [ ] At least one concrete stage-layering invariant emits a `PV####`
      warning via `RenderedDiagnostic::pipeline_violation`, exercised
      by a test.
- [ ] `patches-interpreter::build` consumes a `BoundPatch` produced by
      `descriptor_bind` rather than re-running descriptor resolution
      inline; player/CLAP call sites updated; descriptor-level errors
      surface as `BindError`, not `InterpretError`.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean.

## Tickets

| ID   | Title                                                          |
|------|----------------------------------------------------------------|
| 0435 | Migrate LSP feature handlers to BoundPatch descriptor lookups  |
| 0436 | Bucket pipeline diagnostics by source URI                      |
| 0437 | Emit PV#### pipeline-layering warnings                         |
| 0438 | Consume BoundPatch inside patches-interpreter::build           |

## Notes

0436 is the only ticket here with a sequencing constraint — it should
land alongside or after 0433 (tree-sitter fallback gating) so the
bucketed diagnostics don't collide with the existing TS-emitted
semantic diagnostics on clean-pest files.

0438 is the Q1c decision recorded in the 0432 planning conversation:
the descriptor-bind pass was deliberately scoped to LSP first; lifting
`build` onto it is a standalone refactor with its own risk profile
(player/CLAP hot-reload paths must stay fail-fast).
