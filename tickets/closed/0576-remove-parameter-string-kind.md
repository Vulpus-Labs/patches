---
id: "0576"
title: Remove ParameterKind::String and ParameterValue::String entirely
priority: medium
created: 2026-04-19
---

## Summary

Remove the `String` variant from both `ParameterKind` and
`ParameterValue`. No shipping module declares `ParameterKind::String`
(audit 2026-04-19); the variant, its supporting infrastructure, and
the interpreter resolution path are pure dead weight. Removing them
shrinks the parameter-value type to exactly the set supported on the
audio-thread path (`Float`, `Int`, `Bool`, `Enum`, `File`,
`FloatBuffer`) and eliminates a category of "do I need to handle
String here" questions from every subsequent spike.

This ticket is scoped to the removal only. It does not address
`File` → `FloatBuffer` conversion (Spike 2) or any audio-thread
handling of `FloatBuffer` (Spike 2+).

## Acceptance criteria

- [ ] `ParameterValue::String` variant removed from
      `patches-core/src/modules/parameter_map.rs`.
- [ ] `ParameterKind::String` variant removed from
      `patches-core/src/modules/module_descriptor.rs`, along with
      any `string_param` builder helper.
- [ ] `Module::default_value` / `kind_name` / `validate_parameters`
      match arms for `String` removed.
- [ ] Interpreter (`patches-interpreter/src/tracker.rs` and
      `src/lib.rs`) no longer produces `ParameterValue::String`.
      `Scalar::Str` resolves only against `ParameterKind::Enum`; an
      `Str` paired with any non-enum kind is a clean
      `ParamConversionError::TypeMismatch` (or equivalent).
- [ ] FFI JSON codec
      (`patches-ffi-common/src/json/{ser,de,mod}.rs`) no longer
      handles `String` variants; the `"label"` test fixture is
      removed or reworked.
- [ ] LSP completions (`patches-lsp/src/completions/mod.rs`) no
      longer reference `ParameterKind::String`.
- [ ] Docs (`docs/src/implementing-modules.md`, any generated
      reference) updated to remove mentions of string params.
- [ ] `cargo test` green across the workspace. `cargo clippy`
      clean. `cargo build` clean with no dead-code warnings tied
      to the removed variants.
- [ ] A grep for `ParameterKind::String` and `ParameterValue::String`
      across the repo returns only this ticket and ADR 0045
      (which documents the removal).

## Notes

Ordering within E096: depends on ticket 0574 (the enum migration),
because 0574 leaves the interpreter's `Scalar::Str` resolution in
a state that still has a `String` arm — easier to remove that arm
as a coherent patch after the enum logic has settled.

No effect on the audio-thread path — neither variant ever reached
the audio thread in production. This is a cleanup, not a
correctness fix.

## Out of scope

- `File` removal or conversion (Spike 2).
- Any further reshape of `ParameterValue` (e.g. to
  `FloatBufferId`) — Spike 2.
