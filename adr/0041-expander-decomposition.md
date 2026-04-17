# ADR 0041 — Decompose the DSL template expander

**Date:** 2026-04-17
**Status:** proposed

## Context

[patches-dsl/src/expand/mod.rs](../patches-dsl/src/expand/mod.rs) is
1216 lines. Sibling modules already carve out parsing-adjacent phases
— [composition.rs](../patches-dsl/src/expand/composition.rs) (song/
pattern assembly), [connection.rs](../patches-dsl/src/expand/connection.rs)
(port-index primitives), [scope.rs](../patches-dsl/src/expand/scope.rs)
(lexical scopes), [error.rs](../patches-dsl/src/expand/error.rs)
(diagnostic types). What remains in `mod.rs` is the `Expander<'a>`
struct and its `impl` block (~860 lines), plus ~110 lines of free
helpers.

Ticket 0507 (E086) attempted a split by extracting the error and
scope types into siblings but kept the `Expander` impl intact, landing
at 1210 lines (target: 600). The 0507 close note recorded the reason:
the ticket's axes did not authorize splitting the impl itself, and
the `Expander` impl is a single recursive orchestrator that resists
naive method-by-method extraction.

The file is central — every change to DSL semantics lands here — and
its size means edits routinely touch material unrelated to the change
at hand. It is also the hardest code in the workspace to test in
isolation: its logic (template argument binding, alias scoping,
recursion guarding) is only reachable via full-file expansion.

Two adjacent pressures push the same way:

- **ADR 0040 (kernel carve)** makes `patches-dsl` part of the stable
  kernel that external plugin projects will depend on. A clean
  expander internals surface reduces the blast radius of future
  changes for out-of-tree consumers.
- **DSL evolution** (group params, aliases, arity expansion, `@`-block
  index binding) keeps adding call-site forms that all funnel through
  `expand_template_instance`. Without structural decomposition, every
  new form stretches the same 274-line method.

## Decision

Decompose the expander in four tiers, landing in order. Each tier is
independently valuable and leaves the crate's public surface
unchanged (`patches_dsl::expand::expand`, `ExpandError`, `ExpandResult`,
`Warning`). Each tier closes behaviour-preserving; integration tests
(`expand_tests`, `torture_tests`, `structural_tests`) pass at every
boundary.

### Tier 1 — file-level split

Spread `impl Expander<'a>` across sibling files. Rust allows multiple
`impl` blocks for the same type; descendants of the module that
declares the type see its private fields. `Expander` moves into
`expand/expander/mod.rs` and its methods spread across:

- `substitute.rs` — `subst_scalar`, `subst_value`, `eval_shape_arg_value`,
  `expand_param_entries_with_enum`
- `passes.rs` — `expand_body`, `expand_body_scoped`, and the four
  `pass_*` walkers
- `template.rs` — `expand_template_instance`
- `emit.rs` — `expand_connection`, `emit_single_connection`

Mechanical. No signature change. Resolves the E086 overshoot
flagged in 0507.

### Tier 2 — stateless methods to free functions; manual scope guards to RAII

Two cheap wins become reachable once tier 1 is in place:

**Stateless substituters.** `subst_scalar`, `subst_value`,
`eval_shape_arg_value`, and `expand_param_entries_with_enum` never
read `Expander`'s fields. They take `param_env` as a parameter and
return. They are methods only by convention. Moving them to free
functions in `expand/substitute.rs` (sibling to `connection.rs` / `scope.rs`,
not under `expander/`) reduces the impl surface by ~110 lines and
makes the "pure AST→AST rewrite" layer legible as such.

**RAII scope guards.** Two places do push-call-pop by hand:

- `expand_body` swaps `self.alias_maps` with a fresh map, calls
  `expand_body_scoped`, restores the old map.
- `expand_template_instance` inserts into `self.call_stack`, calls
  `self.expand_body(...)`, removes after.

Today these work because `?` lands before the restore. They are
fragile — any future refactor that changes the control flow around
the recursive call risks leaking a stale guard entry into a sibling
frame.

Replace with `AliasMapFrame<'e>` and `CallGuard<'e>` Drop-guards that
take `&mut Expander`, perform the push in `new`, and the pop in
`Drop::drop`. Makes the scope lifecycle structural.

### Tier 3 — decompose `expand_template_instance`

The 274-line method is five sequential phases disguised as one
function:

1. Recursion-guard check
2. **Argument classification** — walk `decl.shape` into
   `scalar_call_params`; walk `decl.params` into `group_calls`
   (broadcast / array / per-index / `@`-block / arity)
3. **Scalar param binding** — resolve each declared scalar param
   against `scalar_call_params` or its default; type-check
4. **Group param expansion** — resolve arity `N`; emit `name/i`
   slots from `group_calls` or defaults; type-check
5. Recurse into `expand_body` with the new child context

Each phase consumes the previous phase's output and produces data for
the next. This is a pipe of pure transformations wrapped in a single
method.

Extract as free functions:

```rust
fn classify_call_args(decl, template, param_env, alias_map)
    -> Result<(ScalarCallParams, GroupCalls), ExpandError>;

fn bind_template_params(template, scalar_calls, group_calls, span)
    -> Result<(ParamEnv, ParamTypes), ExpandError>;
```

`expand_template_instance` shrinks to ~50 lines of orchestration
(guard, classify, bind, recurse). The param-binding logic becomes
reachable in unit tests without constructing a full file AST.

### Tier 3b — remove `alias_maps` as a global field

`Expander::alias_maps` is conceptually per-body: a module's alias
map is only meaningful to connections in the same body. It is a
global only because the save/restore dance in `expand_body` avoids
cross-body leakage.

Replace the field with a parameter threaded through the pass methods.
`expand_body` constructs the map, `pass_modules` populates it,
`pass_connections` reads it, drop at scope exit. `expand_body_scoped`
becomes unnecessary (its only reason to exist is the save/restore
split).

### Tier 4 — bundle `ExpansionCtx` + `BodyState` as `BodyFrame`

Every pass today takes both an `ExpansionCtx<'_, '_>` (immutable
borrows) and a `BodyState` (mutable accumulator). They are two halves
of the same concept — the per-body frame.

Bundle as `BodyFrame<'ctx, 'a>` with emitter methods
(`emit_module`, `emit_connection`, `record_port_ref`). Pass bodies
become:

```rust
for stmt in stmts {
    translate_module(stmt, &mut frame)?;
}
```

where `translate_module` is a free function in `passes.rs`.
`Expander` keeps only `templates` (and, if tier 3b not yet done,
`call_stack`). The expander effectively becomes a walker; translation
logic lives in free functions per statement kind.

This is the point at which the `Expander` type could plausibly be
dropped entirely in favour of threaded state — though doing so is
out of scope for this ADR.

## Rationale

**File size alone is not the problem.** The problem is that
`expand_template_instance` is the only entry point to the
template-argument binding logic, and the binding logic is the most
complex part of the expander. Tier 3 (extract binding as pure
functions) is the payoff. Tiers 1, 2, and 3b remove obstacles that
otherwise make tier 3 risky or noisy: tier 1 separates files so
diffs are reviewable; tier 2 moves stateless code out of the way so
the remaining impl is only stateful orchestration; tier 3b removes
the mutable global that makes "pure functions" dishonest.

**Ordering is load-bearing.** Doing tier 3 without tier 2 means
binding functions would still reach into `self.alias_maps` and
`self.call_stack`. Doing tier 3b before tier 2 means refactoring
passes twice (once for alias_maps threading, once when stateless
methods move out). Tier 4 is most visible but mechanically cheapest
after tier 3b.

**Why not stop at tier 1.** Tier 1 is purely cosmetic. It addresses
file length but leaves `expand_template_instance` monolithic and
untestable. File-level carving without behavioural carving is what
0507 delivered; repeating the same shape would not make the
expander more legible to readers or more amenable to future DSL
features.

**Why not rewrite wholesale.** The expander's recursion pattern,
scope guards, and argument-classification logic have accreted through
many tickets (aliases, `@`-blocks, arity expansion, group params,
song/pattern-typed params). A rewrite would mean re-deriving those
semantics from the test suite. Incremental decomposition preserves
them.

## Consequences

**Positive**

- Every tier leaves the public surface and all integration tests
  untouched.
- After tier 2, the "pure AST-rewrite" substrate lives alongside
  `composition.rs` and `connection.rs` where it thematically
  belongs, rather than as methods on an orchestrator.
- After tier 3, template-argument binding becomes unit-testable in
  isolation — the hardest semantics in the DSL gain direct test
  coverage.
- After tier 3b, the save/restore bug class vanishes (cannot leak
  state that is no longer stored on the orchestrator).
- After tier 4, adding a new statement kind means adding a free
  `translate_<kind>` function, not extending `Expander`.

**Negative**

- Four-tier sequence. Each tier is a PR; total lead time is weeks,
  not days. Mitigated by each tier landing independently.
- Tier 2's RAII guards introduce two new types whose only purpose
  is correctness-by-construction. Readers must recognise the
  pattern. Accepted — it's a standard Rust idiom.
- Tier 3 and 3b together change the signature of every pass method.
  Merge conflicts in flight-work that touches expander internals are
  likely; coordinate scheduling.

**Neutral**

- Not tied to ADR 0040 (kernel carve); expander internals are
  private to `patches-dsl` regardless of where the crate sits.
- Integration tests (`expand_tests`, `torture_tests`,
  `structural_tests`) are unchanged — the public entry point and
  its behaviour are the contract this ADR commits to preserving.

## Blast radius

### Tier 1

- Create `expand/expander/{mod,substitute,passes,template,emit}.rs`.
- `expand/mod.rs` shrinks to ~250 lines (entry point + shared
  helpers).
- Sibling modules (`composition`, `connection`, `scope`, `error`)
  untouched.
- Import paths inside `expander/` resolve primitives via
  `super::super::{composition,connection,scope}::*`.

### Tier 2

- New file `expand/substitute.rs` (free functions, ~110 lines).
- New types `AliasMapFrame`, `CallGuard` in `expand/expander/mod.rs`
  (small — Drop impls + constructors).
- Callers of substituters inside `expand/expander/*` swap
  `self.subst_*(...)` for `substitute::subst_*(...)`.

### Tier 3

- New free functions `classify_call_args`, `bind_template_params`
  (in `expand/expander/template.rs` or a new `expand/binding.rs`).
- `expand_template_instance` shrinks to ~50 lines.
- New intermediate types (`ScalarCallParams`, `GroupCalls`,
  `BoundTemplateParams`) — `pub(super)` only.
- First meaningful unit-test opportunity — binding behaviour without
  going through `expand()`.

### Tier 3b

- `Expander::alias_maps` field removed.
- `expand_body_scoped` collapses into `expand_body`.
- `pass_modules`, `pass_connections`, `expand_connection`,
  `emit_single_connection` gain `alias_maps: &mut AliasMap` (or
  `&AliasMap` for the read side).

### Tier 4

- New type `BodyFrame<'ctx, 'a>` bundling `ExpansionCtx` and
  `BodyState`.
- Pass methods become free functions `translate_module`,
  `translate_connection`, `translate_song`, `translate_pattern`
  taking `&mut BodyFrame`.
- `Expander` reduces to `{ templates, call_stack }` — or, post-3b,
  `{ templates }`.

## Alternatives considered

- **File split only (tier 1 alone).** Would close ticket 0540 but not
  improve testability or legibility meaningfully. Rejected: repeats
  the 0507 shape without learning from it.
- **Extract a `Substituter` struct / trait.** Overkill for Rust;
  free functions (tier 2) do the job without introducing a type
  whose only purpose is bundling stateless methods.
- **Visitor pattern with `StatementTranslator` trait.** Each
  statement kind would impl the trait. Rejected: no polymorphism
  is actually needed — the four statement kinds have no shared
  return type and the current match is the clearest expression.
- **Full rewrite.** Rejected — semantics live in the test suite but
  are finicky; incremental decomposition preserves them without
  re-derivation.
