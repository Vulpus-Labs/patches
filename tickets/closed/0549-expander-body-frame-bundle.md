---
id: "0549"
title: Bundle per-body state into BodyFrame (tier 4a of expander decomposition)
priority: medium
created: 2026-04-17
epic: E091
depends-on: "0548"
---

## Summary

Tier 4a of [ADR 0041](../../adr/0041-expander-decomposition.md) folds the
three per-body locals owned by `expand_body` —
`ExpansionCtx`, `BodyState`, and `AliasMap` — into a single
`BodyFrame<'ctx, 'a>` bundle. The four pass methods on
[Expander](../../patches-dsl/src/expand/expander/mod.rs#L20-L29) shrink
from "context + state + alias_map + assorted mutable refs" signatures to
"`&mut BodyFrame, &mut Expander`". Lifts the orchestration burden off
[expand_body](../../patches-dsl/src/expand/expander/passes.rs#L30-L52)
in preparation for tier 4b, where the pass methods become free
translator functions.

This ticket does not move pass methods out of `impl Expander` — that is
tier 4b. The pass methods stay as methods; only their parameter list
changes.

## Why now

After tier 3b, `expand_body` constructs three locals (`alias_map`,
`scope`, `state`), then threads them into four pass methods with
hand-written argument lists that drift in size as the passes evolve. The
template-instance branch of `pass_modules` already reaches into seven
fields of `BodyState` plus the alias map plus the context. Bundling
these into one frame is a precondition for tier 4b — free translator
functions need a single mutable receiver so their signatures stay sane
as the codebase evolves.

`scope` (a `NameScope`) is *also* per-body but it lives in
`ExpansionCtx` after construction (it is what `parent_scope` becomes
for the child). Constructing the scope is the responsibility of
`expand_body` because it depends on the body's `stmts`; once built it
moves into the frame.

## Scope

### Types

In [patches-dsl/src/expand/expander/passes.rs](../../patches-dsl/src/expand/expander/passes.rs)
(or a new `expand/expander/frame.rs` — ticket-author's call; whichever
keeps the relevant module under ~400 lines):

```rust
pub(in crate::expand) struct BodyFrame<'ctx, 'a: 'ctx> {
    pub(in crate::expand) ctx: ExpansionCtx<'ctx, 'a>,
    pub(in crate::expand) scope: NameScope<'a>,
    pub(in crate::expand) state: BodyState,
    pub(in crate::expand) alias_map: AliasMap,
}

impl<'ctx, 'a: 'ctx> BodyFrame<'ctx, 'a> {
    fn new(stmts: &[Statement], ctx: ExpansionCtx<'ctx, 'a>) -> Self;
    fn into_body_result(self) -> BodyResult;
}
```

`BodyFrame::new` does the work currently inlined in `expand_body`:
constructs `NameScope::child`, fresh `AliasMap`, fresh `BodyState`.
`into_body_result` packages the populated `state` and `state.boundary`
into the existing `BodyResult` shape.

Field visibility stays `pub(in crate::expand)` so tier 4b can lift
translators into free functions in the same module without forcing
accessor methods.

### Pass method signatures

In [patches-dsl/src/expand/expander/passes.rs](../../patches-dsl/src/expand/expander/passes.rs):

```rust
fn pass_modules(&mut self, stmts: &[Statement], frame: &mut BodyFrame)
    -> Result<(), ExpandError>;
fn pass_connections(&mut self, stmts: &[Statement], frame: &mut BodyFrame)
    -> Result<(), ExpandError>;
fn pass_songs(&mut self, stmts: &[Statement], frame: &mut BodyFrame)
    -> Result<(), ExpandError>;
fn pass_patterns(&mut self, stmts: &[Statement], frame: &mut BodyFrame);
```

Internal mutations switch from `state.flat_modules.push(...)` to
`frame.state.flat_modules.push(...)`; alias-map reads/writes go through
`frame.alias_map`; ctx accesses go through `frame.ctx`.

`expand_body` becomes:

```rust
pub(in crate::expand) fn expand_body(
    &mut self,
    stmts: &[Statement],
    ctx: ExpansionCtx<'_, '_>,
) -> Result<BodyResult, ExpandError> {
    let mut frame = BodyFrame::new(stmts, ctx);
    self.pass_modules(stmts, &mut frame)?;
    self.pass_connections(stmts, &mut frame)?;
    self.pass_songs(stmts, &mut frame)?;
    self.pass_patterns(stmts, &mut frame);
    Ok(frame.into_body_result())
}
```

Note the `ctx` parameter changes from `&ExpansionCtx<'_, '_>` to owned
`ExpansionCtx<'_, '_>`. `ExpansionCtx` is a struct of `&` borrows so
moving it is cheap; `BodyFrame` needs to own it because the frame
outlives the synthetic borrow `expand_body` would otherwise have. All
four current callers of `expand_body` (root expand in
[mod.rs:80](../../patches-dsl/src/expand/mod.rs#L80) and the recursive
call in [template.rs:70](../../patches-dsl/src/expand/expander/template.rs#L70))
construct the ctx as a local, so `ExpansionCtx::for_template(...)` ⇒
move is a one-line edit per call site.

### `expand_template_instance` ripple

In [template.rs](../../patches-dsl/src/expand/expander/template.rs):

- The `&mut AliasMap` parameter goes away; the function takes
  `frame: &mut BodyFrame` instead and writes the instance alias map via
  `frame.alias_map.insert(...)`.
- The function still returns `BodyResult` (it is the recursive call
  into the *child* body, which has its own frame). The recursive
  `self.expand_body(&template.body, child_ctx)` call moves the
  child ctx in, gets back a `BodyResult`, returns it.

`pass_modules`'s template branch becomes:

```rust
let sub = self.expand_template_instance(decl, frame)?;
frame.state.flat_modules.extend(sub.modules);
// ...etc, same field-by-field merge as today
```

### `expand_connection` ripple

In [emit.rs](../../patches-dsl/src/expand/expander/emit.rs):
`expand_connection` currently takes seven `&mut`/`&` parameters
(`instance_ports`, `module_names`, `flat_connections`, `boundary`,
`port_refs`, `alias_map`, plus `ctx`). Replace these with
`frame: &mut BodyFrame` and read through `frame.state.*` /
`frame.alias_map` / `frame.ctx`. The `#[allow(clippy::too_many_arguments)]`
attribute disappears with the long arg list.

`emit_single_connection` keeps its current shape — it operates on
`PortBinding` halves and individual span refs that don't naturally
live on the frame. Pass `frame: &mut BodyFrame` instead of the
five separate references it threads today.

## Acceptance criteria

- [ ] `BodyFrame` struct exists with the four fields named above and
      is the sole per-body-state bundle threaded through `expand_body`.
- [ ] `expand_body` body is under 15 lines.
- [ ] `pass_modules`, `pass_connections`, `pass_songs`, `pass_patterns`
      each take exactly two parameters: `stmts: &[Statement]` and
      `frame: &mut BodyFrame`. `&self` / `&mut self` is the receiver
      and does not count.
- [ ] `expand_template_instance` takes `&mut BodyFrame` (no separate
      `ctx`, no separate `alias_map`).
- [ ] `expand_connection` takes `&mut BodyFrame` and no longer carries
      the `#[allow(clippy::too_many_arguments)]` attribute.
- [ ] `emit_single_connection` takes `&mut BodyFrame` in place of
      `instance_ports`, `flat_connections`, `boundary`, `port_refs`,
      `ctx`.
- [ ] Public surface unchanged:
      `patches_dsl::expand::{expand, ExpandError, ExpandResult, Warning}`.
- [ ] `cargo build -p patches-dsl`, `cargo test -p patches-dsl`,
      `cargo clippy` clean workspace-wide.
- [ ] `expand_tests`, `torture_tests`, `structural_tests` pass with
      unchanged counts and unchanged messages.
- [ ] No file in `patches-dsl/src/expand/` exceeds ~400 lines.

## Scope boundary

**In scope:** `BodyFrame` struct, parameter consolidation through the
four passes, `expand_template_instance`, `expand_connection`, and
`emit_single_connection`. Comment trimming on `expand_body` to reflect
the new shape.

**Out of scope:**

- Lifting pass methods out of `impl Expander` (tier 4b, ticket 0550).
- Adding `frame.emit_module(...)` / `frame.emit_connection(...)` thin
  wrapper methods. Direct field access stays — the goal is to bundle
  the parameters, not invent an accumulator API. If 4b finds a real
  reuse case for one, add it then.
- Changing the per-body field set (e.g. moving `module_names` or
  `instance_ports` out of `BodyState`, dropping `boundary` in favour
  of returning a tuple).
- Changing the four-pass schedule, error codes, or error messages.

## Notes

After this ticket, the pass methods read like translators that happen
to be hung off `Expander` for receiver convenience. Tier 4b (ticket
0550) lifts them to free `translate_*` functions taking
`(stmt, &mut BodyFrame, &mut Expander)`. The split exists because the
parameter consolidation is mechanical and reviewable on its own;
hoisting methods to free functions touches a different axis (visibility,
imports, call-site syntax) that benefits from landing against a clean
parameter list.
