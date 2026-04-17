---
id: "E086"
title: Structural impl splits for files >600 lines
created: 2026-04-16
closed: 2026-04-16
status: closed
tickets: ["0497", "0498", "0499", "0500", "0501", "0502", "0503", "0504", "0505", "0506", "0507", "0508", "0509", "0510", "0511"]
---

## Summary

Tier B follow-on to E085. With inline test modules already pulled
out in E085 (tickets 0474–0491), 15 source files remain over ~600
lines on the impl side alone. Each has a natural structural split
— by grammar node (parser), AST section (ast_builder), variant
type (mixer), concern (workspace), error surface (interpreter /
descriptor_bind), etc.

Splits are structural, not cosmetic: each ticket names the
boundary and leaves the crate's public surface untouched. Order
within the epic is independent; tickets can land in any sequence.

## Acceptance criteria

- [x] All 15 tickets (0497–0511) closed. 14 split along their named
      boundary; 0504 (`patches-clap/src/plugin.rs`) deferred — the
      file has no CLAP parameter callbacks to factor into a
      `params.rs`, and MIDI/transport events are ~20 inline lines
      inside the per-sample `plugin_process` loop, so the ticket's
      proposed `params / audio / events` axis had no clean surface
      to cut on. Permission to defer is in the ticket body itself.
- [x] Every listed source file split along the boundary called out
      in its ticket, with module wiring (`mod` declarations,
      re-exports) preserving the existing public API. Exceptions
      recorded on the tickets: `0507` expand/mod.rs landed at ~1210
      lines (vs the ~600 target) because the `Expander` impl alone
      is ~860 lines and splitting it further would go beyond the
      ticket's own proposed axes; `0508` ast_builder/mod.rs
      landed at 519 lines (vs ~300) because the tolerant AST test
      suite exercises the public `build_ast` entry and moving the
      tests elsewhere would add `pub(super)` plumbing without
      readability gain.
- [x] `cargo build`, `cargo test`, `cargo clippy` clean at each
      ticket boundary and across the workspace at epic close.
- [x] No public API changes (crate-external signatures unchanged;
      `pub(crate)` shuffles allowed).
- [x] Histogram rebaselined — no source file sits over ~600 lines
      purely due to a missed structural split addressed here.
      Residuals above 600: `patches-dsl/src/expand/mod.rs` (~1210,
      recursive template expander impl — see 0507), and the
      ticket-0504 deferral noted above.

## Notes

Out of scope:

- Further test-file category splits for `tests/*.rs` (tracked as
  E087).
- Behaviour change, renaming, or public-API adjustment.
- Borderline files (`patches-dsp/src/fft.rs`, `patches-wasm/src/loader.rs`,
  `patches-clap/src/plugin.rs` sub-700) are deferred unless a
  ticket explicitly targets them — revisit after rebaseline.

Pattern reference: where a file is already a flat module, convert
`foo.rs` to `foo/mod.rs` + sibling submodules, re-exporting any
symbols the crate previously accessed from `foo`. Where a file
already lives in a directory (e.g. `expand/mod.rs`,
`workspace/mod.rs`, `mixer/mod.rs`), add sibling submodules
alongside existing ones.
