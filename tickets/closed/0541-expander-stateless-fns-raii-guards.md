---
id: "0541"
title: Stateless substituters → free fns; RAII scope guards (tier 2 of expander decomposition)
priority: medium
created: 2026-04-17
epic: E091
---

## Summary

Tier 1 (ticket 0540) carved the expander impl across
[patches-dsl/src/expand/expander/](../../patches-dsl/src/expand/expander/)
by concern. Tier 2 of
[ADR 0041](../../adr/0041-expander-decomposition.md) cleans up two
smells the file split left in place:

1. Four methods on `Expander` do not touch `self`. They pretend to
   depend on expander state and obstruct unit-testing of substitution
   logic in isolation.
2. Two flows save/restore mutable fields by hand around recursive
   calls (`expand_body` on `alias_maps`; `expand_template_instance` on
   `call_stack`). Manual push/pop is leak-prone across error returns
   and noisy at the call site.

This ticket does two sub-changes. Either order is fine; landing both
in one PR is preferred since they share a review surface.

## Sub-change 2a — stateless substituters become free functions

Move the four `impl Expander<'a>` methods currently in
[patches-dsl/src/expand/expander/substitute.rs](../../patches-dsl/src/expand/expander/substitute.rs)
out of the impl and into a sibling module
[patches-dsl/src/expand/substitute.rs](../../patches-dsl/src/expand/)
(next to `composition.rs` and `connection.rs`, **not** under
`expander/`). The expander-subtree file deletes.

New free-function signatures:

```rust
pub(super) fn subst_scalar(
    scalar: &Scalar,
    param_env: &HashMap<String, Scalar>,
    span: &Span,
) -> Result<Scalar, ExpandError>;

pub(super) fn subst_value(
    value: &Value,
    param_env: &HashMap<String, Scalar>,
    span: &Span,
) -> Result<Value, ExpandError>;

pub(super) fn eval_shape_arg_value(
    value: &ShapeArgValue,
    param_env: &HashMap<String, Scalar>,
    span: &Span,
) -> Result<Scalar, ExpandError>;

pub(super) fn expand_param_entries_with_enum(
    entries: &[ParamEntry],
    param_env: &HashMap<String, Scalar>,
    decl_span: &Span,
    alias_map: &HashMap<String, u32>,
) -> Result<Vec<(String, Value)>, ExpandError>;
```

Call sites inside `expander/passes.rs` and `expander/template.rs`
change from `self.subst_scalar(...)` to `substitute::subst_scalar(...)`
(or a `use` rename). The `_span` parameter of `subst_scalar` stays
unused — preserved so future error surfaces can cite it without a
signature churn.

Note: this is the first file under `expand/` that is *not* part of
the expander orchestration chain, so the module import in
`expand/mod.rs` gains `mod substitute;` alongside the existing
sibling modules. `expand/mod.rs` does not re-export anything from it;
visibility stays `pub(super)` / `pub(in crate::expand)`.

## Sub-change 2b — RAII scope guards

Introduce two Drop-guards inside `expand/expander/mod.rs`:

```rust
/// Saves and restores `Expander::alias_maps` across a body expansion.
pub(super) struct AliasMapFrame<'e, 'a> {
    expander: &'e mut Expander<'a>,
    saved: HashMap<String, HashMap<String, u32>>,
}

impl<'e, 'a> AliasMapFrame<'e, 'a> {
    pub(super) fn push(expander: &'e mut Expander<'a>) -> Self { /* mem::take */ }
}

impl Drop for AliasMapFrame<'_, '_> {
    fn drop(&mut self) { /* restore */ }
}

/// Tracks the template recursion guard. Errors out if `type_name`
/// is already in the call stack (reuses the existing RecursiveTemplate
/// check); inserts on construction, removes on drop.
pub(super) struct CallGuard<'e, 'a> {
    expander: &'e mut Expander<'a>,
    type_name: String,
}

impl<'e, 'a> CallGuard<'e, 'a> {
    pub(super) fn push(
        expander: &'e mut Expander<'a>,
        type_name: &str,
        span: Span,
    ) -> Result<Self, ExpandError> { /* contains-check + insert */ }
}

impl Drop for CallGuard<'_, '_> {
    fn drop(&mut self) { /* remove */ }
}
```

Rewrite call sites:

- [patches-dsl/src/expand/expander/passes.rs:37-41](../../patches-dsl/src/expand/expander/passes.rs#L37-L41):
  replace the manual `std::mem::take` / reassign pair around
  `expand_body_scoped` with `let _frame = AliasMapFrame::push(self);`.
  `expand_body_scoped` can collapse back into `expand_body` once the
  guard owns the lifetime; that removal is still in scope here.
- [patches-dsl/src/expand/expander/template.rs:40-42](../../patches-dsl/src/expand/expander/template.rs#L40-L42) /
  [patches-dsl/src/expand/expander/template.rs:282-293](../../patches-dsl/src/expand/expander/template.rs#L282-L293):
  fold the `contains`-check, `insert`, and `remove` into one
  `let _guard = CallGuard::push(self, type_name, decl.span)?;` at the
  point the recursion guard needs to be active. The `let sub = ...?;
  self.call_stack.remove(...); sub?` dance disappears — `?` can
  propagate directly because the guard unwinds.

Leave the `self.alias_maps.insert(decl.name.name.clone(), ...)` at
[template.rs:55-58](../../patches-dsl/src/expand/expander/template.rs#L55-L58)
alone for this ticket. That entry is consumed inside the same
instantiation and released by the enclosing `AliasMapFrame` when the
parent body unwinds; tier 3b removes the field entirely.

## Acceptance criteria

- [ ] `expand/expander/substitute.rs` deleted; new
      `expand/substitute.rs` contains the four free functions.
- [ ] No `impl Expander` block defines `subst_*`,
      `eval_shape_arg_value`, or `expand_param_entries_with_enum`.
- [ ] `AliasMapFrame` and `CallGuard` live in
      `expand/expander/mod.rs`, are module-private, and are the only
      sites touching `alias_maps` /`call_stack` push-pop pairs.
- [ ] `grep -n 'std::mem::take' patches-dsl/src/expand/` returns no
      hits inside `expander/`.
- [ ] `grep -n 'call_stack\.\(insert\|remove\)' patches-dsl/src/expand/`
      returns no hits outside the guard impls.
- [ ] `expand_body_scoped` either collapses into `expand_body` or is
      explicitly kept with a one-line justification. Both are
      acceptable — choose whichever leaves `passes.rs` cleaner.
- [ ] Public surface unchanged:
      `patches_dsl::expand::{expand, ExpandError, ExpandResult, Warning}`.
- [ ] `cargo build -p patches-dsl`, `cargo test -p patches-dsl`,
      `cargo clippy` clean workspace-wide.
- [ ] `expand_tests`, `torture_tests`, `structural_tests` pass with
      unchanged counts.

## Sub-change 2c — focussed unit tests

The free-fn carve-out and the RAII guards exercise substitution
logic and scope unwind without going through parse/expand. Add
`#[cfg(test)] mod tests` blocks colocated with the code so the
module-private visibility does not need to widen.

### [patches-dsl/src/expand/substitute.rs](../../patches-dsl/src/expand/substitute.rs)

Cover each free fn against its branches directly:

- `subst_scalar` — `ParamRef` hit (substitutes), `ParamRef` miss
  (passthrough, not an error), non-`ParamRef` scalar (clone-through).
- `subst_value` — `Value::Scalar` wraps the substituted scalar,
  `Value::File` passes through untouched.
- `eval_shape_arg_value` — `AliasList(n)` → `Scalar::Int(n)`;
  scalar path delegates to `subst_scalar`.
- `expand_param_entries_with_enum` — the largest surface and the
  one currently only reached via fixtures. One test per branch:
  - `Shorthand` → `(name, substituted_value)`.
  - `KeyValue { index: None }` → `name`.
  - `KeyValue { index: Some(Literal(i)) }` → `name/i`.
  - `KeyValue { index: Some(Name { arity_marker: true }) }` → fans
    out `name/0..N`, uses `param_env[arity_param]`.
  - `KeyValue { index: Some(Name { arity_marker: false }) }` →
    deref via `alias_map`, emits `name/i`.
  - `AtBlock { index: Literal(n), entries }` → `key/n` per entry.
  - `AtBlock { index: Alias(a), entries }` → deref via alias_map.
  - Errors: missing arity param → `Code::UnknownParam`;
    unknown alias → `Code::UnknownAlias` (via `deref_index_alias`).

### [patches-dsl/src/expand/expander/mod.rs](../../patches-dsl/src/expand/expander/mod.rs)

Cover the guards' unwind behaviour, which the integration tests
don't reach because the success paths work equally well with manual
push/pop:

- `AliasMapFrame::push` saves and restores on drop: seed
  `expander.alias_maps` with sentinel entry, push frame, mutate
  inside frame, drop frame in inner scope, assert sentinel
  restored.
- `AliasMapFrame` on early return: simulate an `?`-style early exit
  inside a closure that owns the frame, assert parent map
  reinstated.
- `CallGuard::push` inserts on construction, removes on drop;
  returns `Err(Code::RecursiveTemplate)` when `type_name` already
  in `call_stack`; original `call_stack` state untouched after an
  error push.
- `CallGuard` unwinds on error in guarded body: construct guard,
  drop via scope exit along an error path, assert `call_stack` no
  longer contains the type name.

Tests live in `#[cfg(test)] mod tests` inside the same files.
Helpers (synthetic `Span`, minimal `ParamEntry` construction)
should be local to the test modules — no new `pub` surface.

## Scope boundary

**In scope:** 2a free-function carve-out; 2b RAII guards; 2c
focussed unit tests on the new fns and guards; optional collapse of
`expand_body_scoped`.

**Out of scope:**

- Decomposing `expand_template_instance` (tier 3).
- Removing `Expander::alias_maps` (tier 3b).
- `BodyFrame` bundle (tier 4).

## Notes

Guards take `&mut Expander` and hold it for their lifetime, so the
enclosing function cannot call other `&mut self` methods while a
guard is alive. The existing code already respects that pattern;
the tier-1 split did not introduce any new interleaved mutations.

`CallGuard::push` returns `Result` because the recursion check fails
before the insert — keeps the guard construction and the error path
in one place and avoids needing a separate `check_not_recursive`
method on `Expander`.

If `substitute.rs` ends up importing `connection::deref_index_alias`,
keep that edge — the alias-deref primitive belongs in `connection`,
and `substitute` calling into it is fine. Do not move
`deref_index_alias` as a drive-by.
