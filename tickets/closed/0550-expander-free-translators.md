---
id: "0550"
title: Pass methods become free translator functions (tier 4b of expander decomposition)
priority: medium
created: 2026-04-17
epic: E091
depends-on: "0549"
---

## Summary

Tier 4b of [ADR 0041](../../adr/0041-expander-decomposition.md) lifts
the four `pass_*` methods on
[Expander](../../patches-dsl/src/expand/expander/mod.rs#L20-L29) into
free `translate_*` functions over a `(&mut BodyFrame, &mut Expander)`
pair. Closes E091.

After tier 4a, the pass methods take exactly two parameters and read
like translators that happen to be methods. This ticket finishes the
job: they become free functions, `Expander` is no longer their
receiver, and `expand_body` is the only thing that ties the four
phases together in one orchestrator.

## Why now

`pass_songs` and `pass_patterns` already do not touch `self` —
they only mutate `frame.state` from `stmts`. `pass_modules` and
`pass_connections` do touch `&mut self`, but only to forward into
`expand_template_instance` and `expand_connection` respectively. Those
two callees keep `&mut self` (they consume `templates` and
`call_stack`); the pass-level wrappers do not need to.

Lifting to free functions makes that distinction explicit at the type
level. It also makes future per-statement-kind translators (e.g. a
hypothetical `Statement::Macro`) drop in as new free functions rather
than new `impl` methods on a struct that should not own them.

## Scope

### Free functions in `passes.rs`

In [patches-dsl/src/expand/expander/passes.rs](../../patches-dsl/src/expand/expander/passes.rs):

```rust
pub(in crate::expand) fn translate_modules(
    stmts: &[Statement],
    frame: &mut BodyFrame<'_, '_>,
    expander: &mut Expander<'_>,
) -> Result<(), ExpandError>;

pub(in crate::expand) fn translate_connections(
    stmts: &[Statement],
    frame: &mut BodyFrame<'_, '_>,
    expander: &mut Expander<'_>,
) -> Result<(), ExpandError>;

pub(in crate::expand) fn translate_songs(
    stmts: &[Statement],
    frame: &mut BodyFrame<'_, '_>,
) -> Result<(), ExpandError>;

pub(in crate::expand) fn translate_patterns(
    stmts: &[Statement],
    frame: &mut BodyFrame<'_, '_>,
);
```

Naming note: `translate_modules` (plural) — each function processes
the whole body's slice for one statement kind, matching the current
`pass_*` naming. The epic's `translate_module` (singular) wording was
shorthand; the per-statement granularity is unchanged.

`expand_body` becomes:

```rust
pub(in crate::expand) fn expand_body(
    &mut self,
    stmts: &[Statement],
    ctx: ExpansionCtx<'_, '_>,
) -> Result<BodyResult, ExpandError> {
    let mut frame = BodyFrame::new(stmts, ctx);
    translate_modules(stmts, &mut frame, self)?;
    translate_connections(stmts, &mut frame, self)?;
    translate_songs(stmts, &mut frame)?;
    translate_patterns(stmts, &mut frame);
    Ok(frame.into_body_result())
}
```

`expand_body` stays a method on `Expander` because its `&mut self`
flows into `translate_modules` / `translate_connections` (which call
`expand_template_instance` / `expand_connection` on the expander).

### Methods that stay on `Expander`

- `expand_body` — the orchestrator; needs `&mut self` to forward to
  the translators that touch the expander.
- `expand_template_instance` — touches `templates` and pushes
  `CallGuard` on `call_stack`.
- `expand_connection` and `emit_single_connection` — currently methods
  for receiver convenience; the body of each does not actually use
  `self`. Lifting these is in scope for this ticket too if it falls
  out cleanly. If the borrow checker complains about lifetime
  intersection with `frame`, leave them as methods and note in the
  PR — they are self-contained and tier 4 has bigger fish.

### Module organisation

`passes.rs` currently houses `impl Expander` with the four pass
methods. After this ticket it houses the four free `translate_*`
functions plus `BodyFrame` (or `BodyFrame` lives in a sibling
`frame.rs` from 4a — same as before). The `impl Expander` block in
`passes.rs` shrinks to just `expand_body`.

If `passes.rs` grows past ~400 lines as a result, split per the file
budget — for example `translate_modules` and the
`expand_template_instance` orchestration sit naturally together, while
`translate_songs` / `translate_patterns` are short and could stay in
`passes.rs` as the main file.

## Acceptance criteria

- [ ] Four free `translate_*` functions exist with the signatures above.
- [ ] `pass_modules`, `pass_connections`, `pass_songs`, `pass_patterns`
      no longer exist as methods on `Expander`.
- [ ] `expand_body` body is under 15 lines and is the only place the
      four-pass schedule appears.
- [ ] `Expander` struct still has exactly two fields (`templates`,
      `call_stack`) — unchanged from tier 3b.
- [ ] Public surface unchanged:
      `patches_dsl::expand::{expand, ExpandError, ExpandResult, Warning}`.
- [ ] `cargo build -p patches-dsl`, `cargo test -p patches-dsl`,
      `cargo clippy` clean workspace-wide.
- [ ] `expand_tests`, `torture_tests`, `structural_tests` pass with
      unchanged counts and unchanged messages.
- [ ] No file in `patches-dsl/src/expand/` exceeds ~400 lines.
- [ ] E091 acceptance criteria all satisfied; epic moves to closed.

## Scope boundary

**In scope:** lifting the four pass methods to free functions in
`passes.rs`; trimming `expand_body`; epic close.

**Out of scope:**

- Removing `Expander` as a struct. Tier 4 retains the type — it is
  the natural home for `templates` + `call_stack` and the receiver
  for `expand_template_instance` / `expand_connection`.
- Splitting `BodyState` into per-pass sub-bundles.
- Restructuring `expand_template_instance` further (its tier-3 shape
  is the target).
- Changing pass order, error codes, or error messages.

## Notes

E091 closes on this ticket. After it lands:

- `expand/expander/mod.rs` is ~50 lines (struct + ctor + `CallGuard`).
- `expand/expander/passes.rs` houses `expand_body` + four free
  translators.
- `expand/expander/template.rs` houses `expand_template_instance`.
- `expand/expander/emit.rs` houses connection emit.
- `expand/binding.rs` houses the binding pipeline.
- `expand/substitute.rs` houses the stateless substituters.

Per ADR 0041, the four-tier decomposition is complete and the
expander's recursive descent is built out of unit-testable free
functions over a small, explicit shared state.
