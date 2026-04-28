---
id: "0743"
title: Extract validate_and_pack and drop Module::update_parameters default
priority: high
created: 2026-04-28
epic: "E126"
parent: "0734"
adrs: ["0060"]
---

## Summary

Third slice of 0734 (ADR 0060). Lift the validate-then-pack logic
out of `Module::update_parameters`'s default trait impl into a free
function `validate_and_pack(descriptor, &ParameterMap) ->
Result<ParamFrame, BuildError>` in `patches-core`. Delete the
`update_parameters` default method. `Module::build` calls the free
function and then `update_validated_parameters` directly.

## Acceptance criteria

- [ ] Free function `validate_and_pack` added in `patches-core`
      (likely `param_frame` or `modules::module`). Encapsulates the
      `validate_parameters` + `compute_layout` + `pack_into` sequence
      currently inlined in `Module::update_parameters`'s default impl.
- [ ] `Module::update_parameters` removed from the trait. Callers
      that invoked it (e.g. test sites) switch to the free function +
      `update_validated_parameters`.
- [ ] `Module::build` default impl uses the free function.
- [ ] `ConvolutionReverb` and any other current overriders of
      `update_parameters` migrate. (The full conv-reverb structural
      migration lands in 0737 — for this ticket, keep its bespoke
      file-resolution code working via the existing
      `File`/`FloatBuffer` route, just routed through the new
      callsite shape.)
- [ ] `cargo test` and `cargo clippy` pass.

## Notes

Depends on 0741 + 0742 landing first. Behaviour unchanged.
