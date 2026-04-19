---
id: "0575"
title: DSL / interpreter / LSP round-trip tests for enum resolution
priority: medium
created: 2026-04-19
---

## Summary

Add regression coverage for DSL source-text → variant index
resolution across every shipping module with enum parameters, and
verify LSP completions still render variant names from the
descriptor after the ticket 0574 migration. This is the
"Spike 0 secure before moving on" bar for ADR 0045.

## Acceptance criteria

- [ ] New integration test (in
      `patches-integration-tests/tests/` or extending
      `dsl_pipeline.rs`) that for each shipping enum parameter:
      1. builds a minimal patch referring to the parameter by
         source-text variant name;
      2. resolves through `parse → expand → interpreter::build`;
      3. asserts the resulting `ParameterValue::Enum(u32)` equals
         the expected variant index;
      4. asserts an invalid variant name produces a clean
         `ParamConversionError::OutOfRange` rather than a panic or
         a silent default.
- [ ] Coverage matrix:
      `oscillator.fm_type`, `poly_osc.fm_type`, `lfo.mode`,
      `drive.mode`, `tempo_sync.subdivision`,
      `fdn_reverb.character`, `convolution_reverb.ir`,
      `master_sequencer.sync`, `vchorus.variant`, `vchorus.mode`.
- [ ] LSP completion test (existing or new in
      `patches-lsp/tests/`) confirms that for each of the above
      modules, completion requests on an enum parameter return
      variant names as labels.
- [ ] Property test (quickcheck-style, in-process) asserting that
      for every `ParameterKind::Enum { variants, .. }`, every
      variant name in `variants` round-trips as
      `name → index → variants[index] == name`.

## Notes

The property test is a cheap belt-and-braces check that catches
any accidental reordering of a descriptor's `variants` slice
relative to its typed enum's discriminants. It must run *after*
ticket 0574 lands (the whole point is to guard against divergence
in the newly-migrated code).

No allocation trap additions here; the audio-thread behaviour is
unchanged at this stage. Allocation-trap extension to cover
`update_validated_parameters` is a Spike 3/5 concern.
