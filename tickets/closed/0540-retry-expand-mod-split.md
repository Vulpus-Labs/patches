---
id: "0540"
title: Spread `impl Expander` across sibling files (tier 1 of expander decomposition)
priority: medium
created: 2026-04-17
epic: E091
retries: "0507"
---

## Summary

[patches-dsl/src/expand/mod.rs](../../patches-dsl/src/expand/mod.rs) is
1216 lines. Ticket 0507 (E086) split the expander's error and scope
types into siblings but kept the `Expander` impl intact; its ~860-line
impl block now dominates the file. [ADR 0041](../../adr/0041-expander-decomposition.md)
lays out a four-tier decomposition; this ticket is tier 1.

Tier 1 is a file-level split only. Rust allows multiple
`impl Expander<'a>` blocks for the same type; descendants of the
module that declares the type see private fields. `Expander` moves
into `expand/expander/mod.rs` and its methods spread across sibling
files by concern. Later tiers (2–4) restructure the methods themselves
— out of scope here.

## Target layout

```text
expand/
  mod.rs            # pub fn expand, BodyResult, ExpansionCtx,
                    # PortBinding, BodyState, top-level helpers
                    # (list_keys, scalar_to_u32, scalar_to_usize)
  composition.rs    # (unchanged)
  connection.rs     # (unchanged)
  error.rs          # (unchanged)
  scope.rs          # (unchanged)
  expander/
    mod.rs          # Expander struct + `fn new` + anything shared
    substitute.rs   # impl: subst_scalar, subst_value,
                    #       eval_shape_arg_value,
                    #       expand_param_entries_with_enum
    passes.rs       # impl: expand_body, expand_body_scoped,
                    #       pass_modules, pass_connections,
                    #       pass_songs, pass_patterns
    template.rs     # impl: expand_template_instance + local helpers
                    # (expand_group_param_value, check_param_type,
                    # build_alias_map move here if used only by this
                    # block — otherwise they stay in expand/mod.rs)
    emit.rs         # impl: expand_connection, emit_single_connection
                    # (orchestrator-side; primitives stay in
                    # sibling `super::super::connection`)
```

## Acceptance criteria

- [ ] `expand/mod.rs` under ~250 lines (entry point + shared
      types/helpers).
- [ ] `Expander` struct declared in `expand/expander/mod.rs`; its
      methods spread across the sibling files listed above as
      multiple `impl Expander<'a> { … }` blocks.
- [ ] No file in the `expander/` subtree exceeds ~400 lines.
- [ ] Existing sibling module paths (`super::composition::*`,
      `super::connection::*`, `super::scope::*`, `super::error::*`)
      continue to resolve — from inside `expander/` they are reached
      as `super::super::...::*`. Adjust imports, not the primitives'
      own paths from outside.
- [ ] `patches_dsl::expand::expand` (the public entry) and the public
      return types remain at their current paths.
- [ ] `cargo build -p patches-dsl`, `cargo test -p patches-dsl`,
      `cargo clippy` clean workspace-wide.
- [ ] No behaviour change; `expand_tests`, `torture_tests`, and
      `structural_tests` pass with the same test counts.

## Scope boundary

**In scope:** file-level carving of the existing `impl Expander<'a>`
block.

**Out of scope (tier 2+):**

- Moving stateless methods off the impl entirely (tier 2a).
- RAII `AliasMapFrame` / `CallGuard` guards replacing manual
  push/pop (tier 2b).
- Decomposing `expand_template_instance` into pure binding functions
  (tier 3).
- Removing `Expander::alias_maps` as a field (tier 3b).
- `BodyFrame` bundle and free translator functions (tier 4).

Resist temptation to do any of these opportunistically — each later
tier has its own acceptance criteria and review surface in E091.

## Notes

E091 tier 1. Retry of 0507. If the 274-line `expand_template_instance`
resists further carving inside `template.rs` alone, that's acceptable
— the target is "no file in the subtree > ~400 lines", not
"no method > ~100 lines". Tier 3 is where that method gets
restructured.

Visibility: private fields of `Expander` (declared in
`expander/mod.rs`) are visible to all submodules of `expander/`.
Moving sibling modules (`composition`, `connection`, etc.) is out of
scope; they stay one level up under `expand/`.
