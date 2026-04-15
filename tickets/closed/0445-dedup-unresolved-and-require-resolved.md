---
id: "0445"
title: Deduplicate UnresolvedModule construction and require-resolved
priority: low
created: 2026-04-15
---

## Summary

Two repeated patterns in `patches-interpreter`:

- `descriptor_bind.rs:382, 399, 416` — three identical blocks building
  `UnresolvedModule { provenance, type_name, shape, params,
  port_aliases, … }` with only the `BindErrorCode` differing.
- `lib.rs:271-299, 303-329, 336-355` — three `build_from_bound`
  branches that check whether a module is `Resolved` and return an
  `InterpretError` otherwise. After ticket 0438, any `Unresolved`
  reaching `build_from_bound` is a caller bug (they should have
  short-circuited on `bound.errors`); the checks are defensive.

Extract:

- `fn mark_unresolved(fm: &FlatModule, code: BindErrorCode) ->
  BoundModule` — one constructor.
- `fn require_resolved<T>(item: T, stage: &str) ->
  Result<ResolvedVariant, InterpretError>` — one guard.

Add a comment at the `require_resolved` sites explaining they are
defensive: the invariant is "caller must have checked bound.errors".

## Acceptance criteria

- [ ] `mark_unresolved` exists; three call sites use it.
- [ ] `require_resolved` (or equivalent helper) exists; three branches
      use it.
- [ ] Doc comment on each helper explains when it should fire in
      practice.
- [ ] `cargo test -p patches-interpreter`, `cargo clippy` clean.

## Notes

Part of E082. Low-priority housekeeping; safe to land any time.
