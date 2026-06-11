---
id: "0998"
title: "DSL robustness: tap-as-source panic, silent cv2 ramp drop, dead error codes"
priority: high
created: 2026-06-11
---

## Summary

1. **Reachable panic in stereo desugar.**
   `patches-dsl/src/stereo_desugar.rs:272` and `:287` call
   `as_port().expect(...)`. `classify()` maps `Tap` / `HostControlRef`
   endpoints to `EndpointKind::Plain` (line 347); the grammar permits taps
   and host-control refs as cable *sources*
   (`cable_endpoint = { tap_target | host_control_ref | port_ref }`), and
   `validate.rs` does not reject tap-as-source. A patch like
   `~ctl -> stereomod.in` (stereo-module destination) panics the compile
   pipeline instead of producing a diagnostic — in a live-coding system
   that's a panic mid-performance.
2. **Silent data loss: cv2 ramp on note steps.**
   `patches-dsl/src/parser/steps_songs.rs:219-224` — the grammar accepts
   `C4:0.5>0.7` but the parser discards the ramp target (`let _ = end;`)
   with no diagnostic. User expects a cv2 ramp, gets a static value.
3. **Dead error codes.** ST0021 (`MissingPatchBlock`), ST0022
   (`MultiplePatchBlocks`), ST0025 (`TapDuplicateName`) are declared in
   `structural.rs` but never raised anywhere. Interpreter BN0009
   (`DuplicateInputConnection`) likewise dead, and the
   `descriptor_bind/mod.rs:33-34` docstring falsely claims
   duplicate-input detection runs there.

## Acceptance criteria

- [ ] Tap / host-control endpoints as cable sources either (a) rejected in
      `validate.rs` with a proper structural error, or (b) handled in the
      desugar arm without `expect`. Decide which is semantically right
      (taps are observation sinks — (a) looks correct); record in the
      ticket on close.
- [ ] Corpus/test: `~ctl -> stereomod.in` and `~meter(x) -> stereomod.in`
      produce diagnostics, not panics (grammar-adjacent: add syntax corpus
      entries per the corpus policy).
- [ ] `value:cv2>target` on note-shaped steps either implemented or
      rejected with a clear error; no silent drop. Test either way.
- [ ] Dead codes ST0021/0022/0025 and BN0009 removed (or documented as
      reserved with a reason); `descriptor_bind` docstring corrected;
      retired-number gaps (ST0026-28, ST0038-39, BN0010-11) get a
      one-line "retired" comment so the registry reads as intentional.

## Notes

The dead `ExpandResult::warnings` infrastructure
(`expand/mod.rs:150`, always empty) is the natural vehicle if cv2-drop
becomes a warning instead of an error — wire it up or remove it, don't
leave it half-present.

## Resolution (2026-06-11)

1. **Tap-as-source** → rejected, option (a). Taps are observation sinks
   with no output. New structural code `TapAsSource` (ST0044), raised by a
   `reject_taps_as_source` pass in `validate.rs` (direction-normalised, so
   it catches `<-` forms too). Runs before `stereo_desugar`, so the
   panicking `as_port().expect` is now unreachable from a tap. The two
   desugar `expect` sites were also hardened to `match … { Some/None }`:
   the surviving non-port source is a **host-control reference**, which is
   a legitimate mono source and now broadcasts directly to both stereo
   sides (no splitter), matching the planner's mono→stereo rule.
2. **cv2 ramp on note steps** → rejected with a parse error in
   `build_step_valued_note` (no more silent `let _ = end;` drop). A cv2
   ramp belongs on a slide step. Fixture `pattern_slides.patches` updated
   (`E4:0.5>0.8` → `E4:0.5`). `ExpandResult::warnings` left untouched —
   the rejection is a hard parse error, not a warning, so no need to wire
   it up here.
3. **Dead codes** removed: ST0021/ST0022/ST0025 (structural) and BN0009
   (interpreter), each replaced with a one-line "retired" comment noting
   the number gap is intentional. `descriptor_bind/mod.rs` docstring
   corrected: fan-in is coalesced into an auto-sum, never rejected as a
   duplicate.

Tests: `tap_validation_tests::{tap_as_cable_source_rejected,
tap_as_source_via_backward_arrow_rejected}`,
`expand::stereo::host_control_source_into_stereo_bus_broadcasts_without_splitter`,
`parser::pattern_song::pattern_cv2_ramp_on_note_rejected`. Syntax-corpus
entries not added: the grammar was unchanged (the rejections are
semantic, at validate/parse-build time), so the corpus's parse-agreement
quadrants don't apply; unit tests are the guard. `just inner -p
patches-dsl` green; clippy clean.
