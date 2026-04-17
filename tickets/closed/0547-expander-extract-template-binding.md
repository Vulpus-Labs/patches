---
id: "0547"
title: Extract template-argument binding as free functions (tier 3 of expander decomposition)
priority: medium
created: 2026-04-17
epic: E091
---

## Summary

Tier 3 of [ADR 0041](../../adr/0041-expander-decomposition.md)
decomposes the ~260-line `expand_template_instance` method in
[patches-dsl/src/expand/expander/template.rs](../../patches-dsl/src/expand/expander/template.rs)
into three pure free functions plus a thin orchestration skeleton.

After tier 2 (ticket 0541), `expand_template_instance` is already
cleaner at its edges — `CallGuard` owns the recursion push/pop, the
substituters are free functions. The remaining 260 lines are the
argument-binding pipeline: call-site classification, param-env
construction, song/pattern-typed param validation. None of that
touches expander state except reading `self.alias_maps` once at the
top to get the current instance's alias map, which is a 3b concern.

This is the tier that makes binding unit-testable in isolation — the
acceptance criterion that ADR 0041 singles out.

## Scope

### Extractions

Three free functions, all `pub(in crate::expand)`:

**`classify_call_args`** — walks the shape block and the param block
of a `ModuleDecl`, classifying each call-site assignment into a
scalar-param binding or a group-param call. Contains the bodies of
[template.rs:68-92](../../patches-dsl/src/expand/expander/template.rs#L68-L92)
(shape walker) and
[template.rs:95-193](../../patches-dsl/src/expand/expander/template.rs#L95-L193)
(param walker).

```rust
pub(in crate::expand) fn classify_call_args(
    decl: &ModuleDecl,
    template: &Template,
    param_env: &HashMap<String, Scalar>,
    alias_map: &HashMap<String, u32>,
) -> Result<(ScalarCallParams, GroupCalls), ExpandError>;
```

Return types:

```rust
pub(in crate::expand) type ScalarCallParams = HashMap<String, Scalar>;
pub(in crate::expand) type GroupCalls =
    HashMap<String, Vec<(Option<usize>, Value)>>;
```

Aliases rather than newtypes — the maps show up at enough call sites
that the names `ScalarCallParams` and `GroupCalls` carry the meaning
without wrapping. Promote to newtypes only if a future caller proves
the need.

**`bind_template_params`** — takes the classified calls and runs
Step 1 (scalar binding with defaults + type check) and Step 2 (group
param expansion per arity). Contains the bodies of
[template.rs:196-211](../../patches-dsl/src/expand/expander/template.rs#L196-L211),
[template.rs:215-245](../../patches-dsl/src/expand/expander/template.rs#L215-L245),
and
[template.rs:248-253](../../patches-dsl/src/expand/expander/template.rs#L248-L253)
(sub_param_types construction).

```rust
pub(in crate::expand) fn bind_template_params(
    template: &Template,
    scalar_calls: ScalarCallParams,
    group_calls: GroupCalls,
    span: &Span,
) -> Result<(HashMap<String, Scalar>, HashMap<String, ParamType>), ExpandError>;
```

`expand_group_param_value` and `check_param_type` (currently free fns
at the bottom of `template.rs`) move with `bind_template_params` or
stay callable from it — no logic change.

**`validate_song_pattern_params`** — checks that any Pattern- or
Song-typed param in the bound env names a real pattern/song in scope.
Contains the body of
[template.rs:256-280](../../patches-dsl/src/expand/expander/template.rs#L256-L280).

```rust
pub(in crate::expand) fn validate_song_pattern_params(
    sub_param_env: &HashMap<String, Scalar>,
    template: &Template,
    scope: &NameScope<'_>,
    decl: &ModuleDecl,
) -> Result<(), ExpandError>;
```

### File layout

Two options:

- **A** — stay in `expand/expander/template.rs`. Keeps binding
  adjacent to its sole caller; file stays around ~400 lines.
- **B** — new sibling `expand/binding.rs` (alongside `substitute.rs`,
  `composition.rs`, `connection.rs`). `binding.rs` knows nothing
  about `Expander`; it operates on AST + param-env types only.

**Recommended: B.** Matches the tier-2 pattern (stateless helpers
live outside `expander/`), shrinks `expand/expander/template.rs` to
the orchestration skeleton (~80 lines), and places the unit tests
next door in `expand/binding/tests.rs`. Add `mod binding;` to
`expand/mod.rs` alongside `mod substitute;`.

### Orchestration skeleton

`expand_template_instance` reduces to roughly:

```rust
pub(super) fn expand_template_instance(
    &mut self,
    decl: &ModuleDecl,
    scope: &NameScope<'_>,
    parent_ctx: &ExpansionCtx<'_, '_>,
) -> Result<BodyResult, ExpandError> {
    let type_name = &decl.type_name.name;
    let template = self.templates[type_name.as_str()];

    // Alias-map for this instance. The insert into self.alias_maps
    // is still here (the enclosing body's connection pass reads it);
    // tier 3b removes the field entirely.
    let instance_alias_map = build_alias_map(&decl.shape);
    let has_aliases = !instance_alias_map.is_empty();
    if has_aliases {
        self.alias_maps.insert(decl.name.name.clone(), instance_alias_map);
    }
    let empty = HashMap::new();
    let alias_map = if has_aliases {
        self.alias_maps.get(decl.name.name.as_str()).unwrap()
    } else {
        &empty
    };

    let (scalar_calls, group_calls) =
        classify_call_args(decl, template, parent_ctx.param_env, alias_map)?;
    let (sub_param_env, sub_param_types) =
        bind_template_params(template, scalar_calls, group_calls, &decl.span)?;
    validate_song_pattern_params(&sub_param_env, template, scope, decl)?;

    let child_namespace = qualify(parent_ctx.namespace, &decl.name.name);
    let child_chain = Provenance::extend(parent_ctx.call_chain, decl.span);
    let child_ctx = ExpansionCtx::for_template(
        Some(&child_namespace),
        &sub_param_env,
        &sub_param_types,
        scope,
        &child_chain,
    );
    let guard = CallGuard::push(self, type_name, decl.span)?;
    guard.expander.expand_body(&template.body, &child_ctx)
}
```

### Unit tests

Direct tests on each extracted function — no `expand()` invocation.
Minimum one happy-path and one error-path per function. Suggested
coverage:

- `classify_call_args`: unknown param name in shape block, group
  param supplied in shape block, scalar param supplied in param
  block, arity-marker param-index form, per-alias param-index form,
  unknown alias name.
- `bind_template_params`: missing required scalar with no default,
  default fallback path, group broadcast form, group per-index form
  with gaps filled by default, group per-index out-of-range, arity
  param not declared before group param.
- `validate_song_pattern_params`: unknown pattern name, unknown song
  name, non-Pattern/Song typed param (should pass through).

Fixture constructors for `Template`, `ModuleDecl`, `NameScope` go
next to the tests; keep them minimal — this is not the place to
build a general-purpose AST builder.

### Error-message and code compatibility

All `StructuralCode` values and error-message wording in the
extracted paths must be preserved. The `structural_tests` suite
matches on `StructuralCode` only, but the torture/expand suites
include message asserts — move, do not rewrite.

## Acceptance criteria

- [ ] `classify_call_args`, `bind_template_params`,
      `validate_song_pattern_params` exist as `pub(in crate::expand)`
      free functions; no `&mut Expander` or `&Expander` parameter.
- [ ] `expand_template_instance` body is under ~60 lines, including
      the alias_map setup that tier 3b will remove.
- [ ] At least one happy-path and one error-path unit test per
      extracted function, in `expand/binding/tests.rs` (or
      equivalent); none invoke `expand()`.
- [ ] `expand_group_param_value` and `check_param_type` are
      reachable from `bind_template_params` with no duplicated
      logic.
- [ ] Public surface unchanged:
      `patches_dsl::expand::{expand, ExpandError, ExpandResult, Warning}`.
- [ ] No file in `patches-dsl/src/expand/` exceeds ~400 lines.
- [ ] `cargo build -p patches-dsl`, `cargo test -p patches-dsl`,
      `cargo clippy` clean workspace-wide.
- [ ] `expand_tests`, `torture_tests`, `structural_tests` pass with
      unchanged counts.

## Scope boundary

**In scope:** the three extractions, new `binding` module if Option
B chosen, direct unit tests, preserving `expand_group_param_value`
and `check_param_type`.

**Out of scope:**

- Removing `Expander::alias_maps` (tier 3b, ticket 0548).
- `BodyFrame` bundle or pass free-function conversion (tier 4).
- Any change to error codes, error-message wording, or warning
  content.
- Refactoring `scope` resolution paths used by
  `validate_song_pattern_params`.
- Promoting `ScalarCallParams` / `GroupCalls` from type aliases to
  newtypes.

## Notes

Tier 3 is the payoff tier of ADR 0041. Everything before it was
structural plumbing; this is where a reader can open `binding.rs`
and understand the template-call binding pipeline without paging
through the rest of the expander.

Tier 3b (ticket 0548) removes `Expander::alias_maps`, which collapses
the `instance_alias_map` setup at the top of
`expand_template_instance` to a single argument threaded from the
enclosing `expand_body`. The tier-3 signatures (`classify_call_args`
takes `alias_map: &HashMap<String, u32>`) are already compatible —
tier 3b only changes where that map comes from at the call site.
