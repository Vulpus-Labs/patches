---
id: "0162"
title: "Expander: resolve ParamRef in all positions, expand shorthand entries"
priority: high
created: 2026-03-20
epic: E027
depends_on: ["0161"]
---

## Summary

Update the template expander to handle the new AST forms from T-0160/T-0161.
`Scalar::ParamRef` replaces `Scalar::Ident` as the substitution target;
`PortLabel::Param` and `Option<Scalar>` scales are resolved to concrete values
during expansion.

## Acceptance criteria

- [ ] `subst_scalar` matches on `Scalar::ParamRef(name)` (was `Scalar::Ident`)
  and substitutes from `param_env`. `Scalar::Str` is returned unchanged (it is
  a literal, not a reference).
- [ ] `expand_connection` resolves `PortLabel::Param(name)` by looking up
  `name` in `param_env` and using the resulting string as the concrete port
  label. Error if the param is absent or its value is not a string-compatible
  scalar.
- [ ] `expand_connection` resolves `Arrow::scale: Option<Scalar>` by calling
  `subst_scalar` and coercing to `f64`. Error if the resolved value is not
  numeric. `None` → 1.0 as before.
- [ ] Shorthand param entries (`ParamEntry::Shorthand(name)`) are expanded to
  `(name, Value::Scalar(Scalar::ParamRef(name)))` before being passed to
  `FlatModule::params`, i.e. they behave identically to `name: <name>`.
- [ ] `Scalar::Str` values pass through expansion unchanged (they are concrete
  string literals, not references). They appear in `FlatModule::params` as
  `Value::Scalar(Scalar::Str(...))` for Stage 3 to interpret.
- [ ] All existing expander tests continue to pass (updated for renamed
  `Scalar::Ident` → `Scalar::Str` / `Scalar::ParamRef` where needed).
- [ ] `cargo clippy -p patches-dsl` and `cargo test -p patches-dsl` pass.

## Notes

The only semantic change is the `Ident` → `ParamRef` / `Str` split. The
substitution logic itself is unchanged; `subst_scalar` just matches a different
variant name.

Port label resolution produces a `String` that goes directly into
`FlatConnection::from_port` / `to_port`. Stage 3 validates it against the
module descriptor as it does for any port name.
