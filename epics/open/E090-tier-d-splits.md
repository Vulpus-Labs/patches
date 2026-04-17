---
id: "E090"
title: Tier D structural splits — post-rebaseline impl and test outliers
created: 2026-04-17
tickets: ["0524", "0526", "0527", "0528", "0529", "0530", "0531", "0532", "0533", "0534", "0535", "0536", "0537", "0538", "0539"]
---

## Summary

Tier D follow-on to E085 (inline test extraction), E086 (impl splits
≥600), and E087 (test-file category splits). After those landed, a
rebaselined histogram shows a fresh long tail:

- 7 impl files ≥550 lines that were borderline or outside earlier tiers
  (patches-wasm/src/loader.rs at 628 is excluded — that crate is parked;
  patches-dsl/src/expand/mod.rs at 1216 is handled separately in E091
  under ADR 0041)
- 8 test files ≥660 lines that were produced by E085's inline-tests
  extraction but never got a category split of their own

Each ticket names a structural boundary — by type (filter variants),
by concern (wasm module vs builder vs loader entry), by port kind
(cable input/output variants), or by test category (cycle tests vs
hover tests). All mechanical, no behaviour change, module surface
preserved.

Scoped to avoid overlap with E089 (kernel carve): `patches-engine/src/{engine,
planner,callback,input_capture}.rs` and `patches-engine/src/builder/mod.rs`
are owned by E089 and excluded here. `patches-engine/src/execution_state.rs`
stays in engine per ADR 0040 and is in scope (0534 was considered but
deferred — see "Out of scope" below).

## Tickets

### Impl splits

| ID   | File                                               | LOC  |
| ---- | -------------------------------------------------- | ---- |
| 0524 | patches-interpreter/src/lib.rs                     | 670  |
| 0526 | patches-dsp/src/drum.rs                            | 596  |
| 0527 | patches-modules/src/filter/mod.rs                  | 587  |
| 0528 | patches-modules/src/convolution_reverb/mod.rs      | 577  |
| 0529 | patches-modules/src/fdn_reverb/mod.rs              | 570  |
| 0530 | patches-core/src/cables/mod.rs                     | 556  |
| 0531 | patches-dsp/src/partitioned_convolution/mod.rs     | 555  |

### Test splits

| ID   | File                                                   | LOC  |
| ---- | ------------------------------------------------------ | ---- |
| 0532 | patches-lsp/src/workspace/tests.rs                     | 1222 |
| 0533 | patches-lsp/src/analysis/tests.rs                      |  785 |
| 0534 | patches-modules/src/master_sequencer/tests.rs          |  774 |
| 0535 | patches-dsp/src/biquad/tests.rs                        |  738 |
| 0536 | patches-interpreter/src/tests.rs                       |  717 |
| 0537 | patches-engine/src/builder/tests.rs                    |  712 |
| 0538 | patches-dsp/src/partitioned_convolution/tests.rs       |  676 |
| 0539 | patches-dsp/src/svf/tests.rs                           |  661 |

## Acceptance criteria

- [ ] All 15 tickets (0524, 0526–0539) closed, each along the boundary
      called out in its ticket.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean at each ticket
      boundary and across the workspace at epic close.
- [ ] No public API changes; `pub(crate)` shuffles allowed.
- [ ] No change to behaviour, fixtures, or the set of test binaries.
- [ ] Histogram rebaselined. Target: no impl file ≥550 lines and no
      test file ≥660 lines remains purely due to a missed split
      addressed here. Exceptions documented on their tickets.

## Patterns

**Impl splits.** Where a file is already a flat module, convert
`foo.rs` to `foo/mod.rs` + sibling submodules. Where a file already
lives in a directory (e.g. `convolution_reverb/mod.rs`), add sibling
submodules alongside existing ones. Re-export any symbols the crate
previously accessed from the original path.

**Test splits.** Same shape as E087. Keep `src/foo/tests.rs` as a stub
that declares `mod tests;` pointing at a `tests/` sibling directory,
with categories as submodule files. Where the host file is
`src/foo/mod.rs` and `tests` is a sibling submodule, structure as:

```text
src/foo/mod.rs             # declares `#[cfg(test)] mod tests;`
src/foo/tests/mod.rs       # declares category submodules + shared helpers
src/foo/tests/<cat>.rs     # one file per axis
```

Category axes are named in each ticket.

## Out of scope

- Files handled by E089 (kernel carve): engine.rs, planner.rs,
  builder/mod.rs, callback.rs, input_capture.rs.
- E086 exceptions: 0504 (clap plugin.rs — no clean boundary) and
  0508 (lsp ast_builder/mod.rs — tests tied to public `build_ast`).
  0507 (dsl expand/mod.rs) is handled in its own epic E091 under
  ADR 0041 — file-level split is tier 1 there; structural
  decomposition is tiers 2–4.
- patches-wasm: parked; `patches-wasm/src/loader.rs` (628) deferred
  until the crate resumes active work.
- Borderline impl 500–549 (modules poly_filter/mod.rs 544, lsp
  server.rs 516, engine execution_state.rs 512, dsl loader.rs 506).
  Revisit after E090 rebaseline if they still stand out.
- Behaviour change, renaming, or public-API adjustment.
- Further inline `mod tests` extraction (all done in E085).
