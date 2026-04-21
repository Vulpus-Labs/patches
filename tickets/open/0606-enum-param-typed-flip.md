---
id: "0606"
title: Flip `enum_param` descriptor builder to typed `EnumParamName<E>`
priority: medium
created: 2026-04-21
---

## Summary

`ModuleDescriptor::enum_param` still takes the legacy string signature
`(name: &'static str, variants: &'static [&'static str], default:
&'static str)`. In parallel we have `enum_param_typed(name:
EnumParamName<E>, default: E)`, which sources variants from
`E::VARIANTS` and the wire default from `default.to_variant()`. Rename
`enum_param_typed` to `enum_param` and drop the string form, so every
enum site uses the same typed path as `float_param` / `int_param` etc.

This closes the last remaining string-based builder signature per
ticket 0605 § Phase B step 5.

## Acceptance criteria

- [ ] `ModuleDescriptor::enum_param` signature is
      `fn enum_param<E: ParamEnum>(self, name: impl Into<EnumParamName<E>>, default: E) -> Self`.
- [ ] `enum_param_typed` removed (or re-exported as a deprecated alias
      if it has external callers — otherwise delete outright).
- [ ] Every in-tree `.enum_param("name", E::VARIANTS, "default")` call
      site rewritten to `.enum_param(params::name, E::Default)`.
- [ ] `From<&'static str>` bridge on `EnumParamName<E>` is *not* added
      — enum names always come from `module_params!` consts.
- [ ] `cargo test` and `cargo clippy --all-targets -- -D warnings`
      clean.

## Notes

Known call sites using the legacy string form at merge of 0605:

- `patches-modules/src/lfo.rs` — `"mode"` / `LfoMode`
- `patches-modules/src/drive.rs`
- `patches-modules/src/oscillator.rs`, `patches-modules/src/poly_osc.rs`
  (check — some may already be typed)
- `patches-modules/src/tempo_sync.rs` — `"subdivision"` / `Subdivision`
- `patches-vintage/src/vchorus.rs` (already typed — sanity check)

`grep -rn "\.enum_param(\"" patches-modules patches-vintage test-plugins`
enumerates the full list at the time this ticket is picked up.

Ticket 0605 noted the trade-off between a proc-macro
`#[derive(ParamEnum)]` and the existing `params_enum!` macro. This
ticket keeps `params_enum!` — it already emits `ParamEnum`. Do not
introduce a new proc-macro crate unless an independent reason
appears.
