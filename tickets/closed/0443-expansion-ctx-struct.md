---
id: "0443"
title: ExpansionCtx struct for expand_body and emit_single_connection
priority: medium
created: 2026-04-15
---

## Summary

Two hot paths in `patches-dsl/src/expand.rs` thread too many parameters:

- `expand_body` takes 9 arguments plus `&mut self`
  (`stmts`, `namespace`, `param_env`, `param_types`, `parent_scope`,
  `call_chain`, …). Callers thread these through untouched.
- `emit_single_connection` takes 14 arguments, including several
  boolean flags (`from_is_arity`, `to_is_arity`, `from_i`, `to_i`).

Refactor into context structs:

- `ExpansionCtx { param_env, param_types, parent_scope, namespace,
  call_chain, … }` — passed into `expand_body` and its callees.
- `PortBinding { port: String, index: u32, is_arity: bool }` — replaces
  the boolean-flag pairs in `emit_single_connection`.

## Acceptance criteria

- [ ] `expand_body` signature is `expand_body(&mut self, stmts: &[Stmt],
      ctx: &ExpansionCtx)` or similar.
- [ ] `emit_single_connection` takes two `PortBinding` values instead
      of six flags + indices.
- [ ] Callers no longer thread identical arguments through multiple
      frames untouched.
- [ ] Expander behaviour is unchanged; all DSL tests pass.
- [ ] `cargo test -p patches-dsl`, `cargo clippy` clean.

## Notes

Part of E082. Pure refactor. Keep `ExpansionCtx` borrow-only where
possible to avoid cloning costs; the existing expander already carries
heavy clone pressure (separate concern, not addressed by this ticket).
