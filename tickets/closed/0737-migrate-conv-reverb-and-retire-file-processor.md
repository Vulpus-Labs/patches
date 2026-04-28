---
id: "0737"
title: Migrate convolution_reverb to ir_path; retire FileProcessor pipeline
priority: high
created: 2026-04-28
epic: "E126"
adrs: ["0060"]
depends_on: ["0734"]
---

## Summary

Migrate `convolution_reverb` (mono and stereo) to declare `ir_path` as
a structural string param. The module reads the path in `prepare`,
performs WAV decode + IR partitioning + FFT pre-processing, and builds
the `NonUniformConvolver` directly. The bespoke `update_parameters`
override is deleted (currently the only such override in the
workspace).

With no remaining consumers of file-as-`FloatBuffer`, retire the
host-side file-resolution pipeline entirely:

- Delete `resolve_file_params` from `patches-planner`.
- Delete the `FileProcessor` trait and its registry entries.
- Delete `ParameterValue::File` and `ParameterValue::FloatBuffer`
  variants and their callers.
- Delete the `FloatBufferId` buffer-slot route through `ParamFrame` /
  `ParamView` (`fetch_buffer_*`, buffer-slot layout, packing logic).

## Acceptance criteria

- [ ] `convolution_reverb` declares `ir_path: structural String` and
      `ir: structural Enum<{builtin, file}>` (or equivalent). IR
      decoding, partitioning, and convolver construction happen
      inside `prepare`. Mono and stereo variants both migrated.
- [ ] `convolution_reverb::update_parameters` override removed.
- [ ] `resolve_file_params`, the `FileProcessor` trait, and its
      registry entries deleted. `Registry::process_file` and any
      related plumbing removed.
- [ ] `ParameterValue::File` and `ParameterValue::FloatBuffer`
      variants removed. `ParameterKind::File` becomes a structural
      string declaration; the realtime path no longer mentions files.
- [ ] `FloatBufferId`, `fetch_buffer_static`, `pack` handling for
      buffer slots, and the `buffer_tail` portion of
      `ParamFrame` layout removed.
- [ ] DSL `file("path.wav")` syntax remains as surface — it now
      desugars to a structural string param. Update `desugar.rs`
      accordingly.
- [ ] Existing `convolution_reverb` tests pass. Add a test that
      structurally edits `ir_path` and confirms the planner triggers
      an instance rebuild (test will rely on 0740 — until then,
      verify only the `prepare` path).
- [ ] `cargo test` and `cargo clippy` pass.

## Notes

This is the largest single deletion in the epic. Confirm before each
removal that no FFI plugin or external crate depends on the symbol.
Run `cargo +nightly udeps` after deletion to catch newly-orphaned
deps.

## Status

Conv-reverb migration to structural `ir_path` (mono + stereo) landed.
The bespoke `update_parameters` override is gone; `apply_unpacked_params`
now consumes a pre-decoded IR stashed in `prepare`. The FloatBuffer
branch in `ConvReverbCore::update_parameters` is removed. Tests pass.

Wholesale pipeline deletion — `resolve_file_params`, `FileProcessor`,
`ParameterValue::File`/`FloatBuffer`, `FloatBufferId`, `fetch_buffer_*`,
`buffer_tail` layout, ArcTable simplification — deferred to **0745**.
With no live consumer left, those symbols are dead but still compile;
0745 deletes them.
