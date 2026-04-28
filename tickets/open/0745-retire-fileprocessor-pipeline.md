---
id: "0745"
title: Retire FileProcessor pipeline and FloatBufferId infrastructure
priority: medium
created: 2026-04-28
epic: "E126"
parent: "0737"
adrs: ["0060"]
depends_on: ["0737"]
---

## Summary

Deferred half of 0737. After conv_reverb migrated to `ir_path`
structural in 0737, no live module declares `ParameterKind::File` in
`realtime_params` and no live module relies on the planner's
file-resolution pipeline. Retire the dead infrastructure.

## Acceptance criteria

- [ ] Delete `resolve_file_params` from `patches-planner`.
- [ ] Delete the `FileProcessor` trait and its registry entries
      (`patches-registry::file_processor`, `Registry::process_file`,
      `Registry::register_file_processor`, `Registry::has_file_processor`).
- [ ] Delete `ParameterValue::File` and `ParameterValue::FloatBuffer`
      variants. Remove all match arms across the workspace.
- [ ] Delete `ParameterKind::File`'s realtime-side packing path; keep
      the variant only insofar as `structural_string_param` uses it,
      or split into a separate `ParameterKind::String` for clarity.
- [ ] Delete `FloatBufferId`, `fetch_buffer_static`, `pack` handling
      for buffer slots, the `buffer_tail` portion of `ParamFrame`
      layout, and corresponding ABI / wire-format support.
- [ ] Delete the FFI `ArcTable` infrastructure entirely
      (`patches-ffi-common::arc_table`, `RuntimeArcTables*`,
      `ArcTableAudio` / `ArcTableControl` handles, counters,
      soak/fuzz tests). With `FloatBufferId` and the realtime
      File→FloatBuffer route gone, ArcTable has no producer and no
      consumer. Existing observability hooks (param-frame dispatch
      counter on `RuntimeAudioHandles`) move to a slimmer
      replacement or fold into ADR 0043's tap surface.
- [ ] `cargo +nightly udeps` clean — no orphaned deps from the
      removal.
- [ ] DSL `file("path.wav")` continues to parse but desugars to a
      structural string param (depends on 0738 for full DSL surface).

## Notes

Largest deletion in the epic. The order that minimises breakage:
1. `resolve_file_params` + `FileProcessor` trait/registry plumbing.
2. `ParameterValue::File` / `FloatBuffer` matches and variants.
3. `ParameterKind::File` realtime path (packer, view, layout).
4. `FloatBufferId` + buffer-slot infrastructure.
5. ArcTable runtime simplification.

Each step ends in a compiling tree.
