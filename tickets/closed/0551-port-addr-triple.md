---
id: "0551"
title: Extract (module, port, index) triple as PortAddr
priority: low
created: 2026-04-17
depends-on: "E091"
---

## Summary

The triple `(module, port, index)` recurs across the DSL flat schema and
the expander's connection emit path. Extract it as a `PortAddr` type to
deduplicate fields and collapse the `m.clone() / p.clone() / *i`
patterns in
[expand/expander/emit.rs](../../patches-dsl/src/expand/expander/emit.rs).

Deferred until E091 closes — the expander decomposition is touching the
same files and a parallel structural change risks merge churn.

## Sites

- [FlatConnection](../../patches-dsl/src/flat.rs#L37-L48) — pair of
  triples + scale + provenance.
- [FlatPortRef](../../patches-dsl/src/flat.rs#L71-L77) — triple +
  direction + provenance.
- `PortEntry = (String, String, u32, f64)` in
  [expand/connection.rs](../../patches-dsl/src/expand/connection.rs) —
  triple + scale; returned by `resolve_from` / `resolve_to`.
- `PortBinding { port, index, is_arity }` — 2/3 of the triple
  (module is held separately by the caller).
- Emit-side construction in
  [expand/expander/emit.rs](../../patches-dsl/src/expand/expander/emit.rs).

## Two flavours

`FlatConnection` / `FlatPortRef` use `QName` for the module; the
emit-time intermediates use `String` (pre-qualification). Either:

- `PortAddr<M> { module: M, port: String, index: u32 }` generic over
  the module type, or
- two structs (`PortAddrQ` / `PortAddrS`) with an explicit conversion.

Generic is one type but pollutes signatures with a parameter; two
structs are noisier but each call site is concrete.

## Scope options

Pick at ticket-start time:

1. **Local only.** Internal `PortAddr` in `expand/connection.rs` +
   `expand/expander/emit.rs`. `FlatConnection` / `FlatPortRef`
   unchanged. Cheap. No consumer churn.
2. **Embed in flat types.** `FlatConnection { from: PortAddr, to:
   PortAddr, scale, ... }`. Cleanest model. Touches every consumer:
   `patches-interpreter`, `patches-lsp`, `patches-svg`, integration
   tests, `flat_to_layout`. Wide blast radius.
3. **Full.** Option 2 plus collapsing `PortBinding` and possibly
   folding the arity flag into `PortAddr` (or keeping it as a
   side-channel on the binding). Biggest surface; clearest model.

## Notes

Field names if extracted: `module` / `port` / `index` (matches the
existing field names on `FlatPortRef`). `FlatConnection`'s `from_*` /
`to_*` prefixes become `from.module` / `to.module` etc. — that rename
is the bulk of the consumer churn for option 2.

No deadline. Revisit after E091 closes.

## Resolution

Scope: Option 1 (local only) plus the PortBinding collapse from
option 3. `FlatConnection` and `FlatPortRef` keep their flat layout —
no consumer churn.

- `PortAddr<M> { module, port, index }` generic over module type,
  defined in
  [expand/connection.rs](../../patches-dsl/src/expand/connection.rs).
- `PortEntry` is now a struct `{ addr: PortAddr<QName>, scale }`
  rather than a four-tuple; `resolve_from` / `resolve_to` construct
  it via `PortEntry::new`.
- `PortBinding` in
  [expand/mod.rs](../../patches-dsl/src/expand/mod.rs) holds
  `PortAddr<String>` + `is_arity`; the `from_module: &str` /
  `to_module: &str` params on `emit_single_connection` are gone.
- `emit.rs` boundary-key formatting, `FlatPortRef` construction, and
  `FlatConnection` construction collapsed into `boundary_key`,
  `port_ref_from_addr`, and `flat_connection` helpers keyed off
  `PortAddr<QName>`.

Acceptance: `cargo build -p patches-dsl`, `cargo test -p patches-dsl`
(36 integration, 58 lib tests), `cargo clippy --workspace` all clean.
