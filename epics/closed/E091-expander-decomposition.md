---
id: "E091"
title: Decompose the DSL template expander
created: 2026-04-17
tickets: ["0540", "0541", "0547", "0548", "0549", "0550"]
---

## Goal

Land the four-tier decomposition of
[patches-dsl/src/expand/mod.rs](../../patches-dsl/src/expand/mod.rs)
described in [ADR 0041](../../adr/0041-expander-decomposition.md).

After this epic:

- `Expander` lives in `expand/expander/mod.rs` and is ~50 lines of
  struct + constructor.
- Pure AST-rewrite helpers live in `expand/substitute.rs` alongside
  `composition.rs` and `connection.rs`.
- Template-argument binding is reachable as free functions with
  unit-testable signatures.
- Alias-map scope is not a mutable global on the expander.
- Each statement kind's translation logic lives in a free
  `translate_<kind>` function.
- No file in the `expand/` subtree exceeds ~400 lines.
- Crate-external surface (`patches_dsl::expand::expand`,
  `ExpandError`, `ExpandResult`, `Warning`) is unchanged at every
  ticket boundary.

## Background

See ADR 0041 for the full rationale. Short version: ticket 0507 (E086)
split the expander's sibling phases (error, scope) but left the
`Expander` impl intact at ~860 lines. The impl is one recursive
orchestrator and resists naive method-by-method extraction; it needs
a structural decomposition, not another file split.

Tier 1 is file-level. Tier 2 moves stateless methods out and
replaces manual push/pop with RAII guards. Tier 3 extracts the
template-argument binding pipeline as pure functions — the real
payoff. Tier 3b removes the alias-map global. Tier 4 generalises
pass methods to free translator functions over a `BodyFrame` bundle.

## Tiers

### Tier 1 — file-level split (ticket 0540, retargeted)

Spread `impl Expander<'a>` across files. Move `Expander` into
`expand/expander/mod.rs`. Sibling files:

- `substitute.rs` — pure substituter methods (they move again in
  tier 2, but landing them here first keeps the PR reviewable)
- `passes.rs` — `expand_body`, `expand_body_scoped`, four `pass_*`
  walkers
- `template.rs` — `expand_template_instance`
- `emit.rs` — `expand_connection`, `emit_single_connection`

Acceptance: `expand/mod.rs` under ~250 lines; no file in
`expander/` exceeds ~400 lines; `cargo build -p patches-dsl`,
`cargo test -p patches-dsl`, `cargo clippy` clean.

### Tier 2 — stateless methods → free functions; RAII scope guards

Two sub-changes, either order:

**2a.** Move the four stateless substituters to free functions in
`expand/substitute.rs` (sibling of `composition.rs` /
`connection.rs`, not under `expander/`):

- `subst_scalar(scalar, param_env, span) -> Result<Scalar, ExpandError>`
- `subst_value(value, param_env, span) -> Result<Value, ExpandError>`
- `eval_shape_arg_value(value, param_env, span) -> Result<Scalar, ExpandError>`
- `expand_param_entries_with_enum(entries, param_env, decl_span, alias_map)
    -> Result<Vec<(String, Value)>, ExpandError>`

Callers inside `expander/` swap `self.subst_*(...)` for
`substitute::subst_*(...)`.

**2b.** Introduce `AliasMapFrame<'e>` and `CallGuard<'e>` Drop-guards
replacing the manual save/restore in `expand_body` and the manual
insert/remove in `expand_template_instance`. Each guard takes
`&mut Expander`, performs the push in a constructor, and the pop in
`Drop::drop`.

Acceptance: no code in `expander/` reaches for manual `std::mem::take`
/ `HashSet::insert` + `HashSet::remove` around recursive calls;
`expand/expander/mod.rs` contains the guard types; integration tests
unchanged.

### Tier 3 — extract template-argument binding

Decompose `expand_template_instance` (currently ~270 lines) into:

```rust
fn classify_call_args(
    decl: &ModuleDecl,
    template: &Template,
    param_env: &HashMap<String, Scalar>,
    alias_map: &HashMap<String, u32>,
) -> Result<(ScalarCallParams, GroupCalls), ExpandError>;

fn bind_template_params(
    template: &Template,
    scalar_calls: ScalarCallParams,
    group_calls: GroupCalls,
    span: &Span,
) -> Result<(HashMap<String, Scalar>, HashMap<String, ParamType>), ExpandError>;
```

Live in either `expand/expander/template.rs` or a new
`expand/binding.rs` — ticket-author's call.

`expand_template_instance` shrinks to the orchestration skeleton:

```rust
let _guard = CallGuard::push(self, type_name)?;  // tier 2b RAII
let (scalar_calls, group_calls) =
    classify_call_args(decl, template, ctx.param_env, alias_map)?;
let (sub_param_env, sub_param_types) =
    bind_template_params(template, scalar_calls, group_calls, &decl.span)?;
validate_song_pattern_params(&sub_param_env, template, scope, decl)?;
let child_ctx = ExpansionCtx::for_template(/* … */);
self.expand_body(&template.body, &child_ctx)
```

Acceptance: `expand_template_instance` under ~60 lines;
`classify_call_args` and `bind_template_params` have unit tests in
`expand/binding/tests.rs` (or similar) that do not invoke
`expand()`; all existing tests still pass.

### Tier 3b — remove `alias_maps` as a global field

Remove `Expander::alias_maps`. Thread the alias map as a parameter.
Shape of change:

- `expand_body` constructs a fresh `AliasMap`, owns it through the
  passes.
- `pass_modules` populates (take `&mut AliasMap`).
- `pass_connections` reads (take `&AliasMap`).
- `expand_connection` and `emit_single_connection` gain `&AliasMap`.
- `expand_body_scoped` disappears — collapse back into `expand_body`
  since the save/restore reason is gone.
- `AliasMapFrame` from tier 2b disappears with its field.

Acceptance: `grep 'self.alias_maps' patches-dsl/src/expand/` returns
empty; `expand_body_scoped` removed; all tests pass.

### Tier 4 — `BodyFrame` bundle; passes become free translators

Bundle the per-body context and accumulator:

```rust
struct BodyFrame<'ctx, 'a> {
    ctx: ExpansionCtx<'ctx, 'a>,
    state: BodyState,
    alias_map: AliasMap,  // from tier 3b
}

impl<'ctx, 'a> BodyFrame<'ctx, 'a> {
    fn emit_module(&mut self, m: FlatModule);
    fn emit_connection(&mut self, c: FlatConnection);
    fn record_port_ref(&mut self, r: FlatPortRef);
    // etc.
}
```

Pass methods become free functions in `expand/expander/passes.rs`:

```rust
fn translate_module(stmt: &Statement, frame: &mut BodyFrame, expander: &mut Expander);
fn translate_connection(stmt: &Statement, frame: &mut BodyFrame, expander: &mut Expander);
fn translate_song(stmt: &Statement, frame: &mut BodyFrame);
fn translate_pattern(stmt: &Statement, frame: &mut BodyFrame);
```

`expand_body` reduces to:

```rust
let mut frame = BodyFrame::new(ctx);
for stmt in stmts { translate_module(stmt, &mut frame, self)?; }
for stmt in stmts { translate_connection(stmt, &mut frame, self)?; }
for stmt in stmts { translate_song(stmt, &mut frame)?; }
for stmt in stmts { translate_pattern(stmt, &mut frame); }
frame.into_body_result()
```

`Expander` holds only `{ templates, call_stack }` at this point.

Acceptance: four free `translate_*` functions exist; `Expander`
holds ≤ 2 fields; `expand_body` under ~40 lines.

## Tickets

| ID   | Tier | Title                                                | Status   |
| ---- | ---- | ---------------------------------------------------- | -------- |
| 0540 | 1    | Spread `impl Expander` across sibling files          | closed   |
| 0541 | 2    | Stateless substituters → free fns; RAII scope guards | closed   |
| 0547 | 3    | Extract template-argument binding as pure functions  | closed   |
| 0548 | 3b   | Remove `Expander::alias_maps` global field           | closed   |
| 0549 | 4a   | Bundle per-body state into `BodyFrame`               | open     |
| 0550 | 4b   | Pass methods become free translator functions        | open     |

Tier-2-onwards ticket text spelled out above so the epic captures
scope even before individual tickets are drafted. Ticket drafting
for tiers 2–4 happens after each preceding tier lands, to reflect
what the code actually looks like at that point.

## Acceptance criteria (epic close)

- [ ] Tier 1 ticket (0540) closed.
- [ ] Tier 2, 3, 3b, 4 tickets drafted, closed, and the listed
      per-tier acceptance criteria met.
- [ ] No file in `patches-dsl/src/expand/` exceeds ~400 lines.
- [ ] `Expander` struct holds ≤ 2 fields.
- [ ] Template-argument binding has direct unit tests in
      `patches-dsl` that do not invoke the top-level `expand()`.
- [ ] Public API unchanged: `patches_dsl::expand::{expand,
      ExpandError, ExpandResult, Warning}` retain their signatures.
- [ ] `cargo build -p patches-dsl`, `cargo test -p patches-dsl`,
      `cargo clippy` clean at each ticket boundary and at epic
      close workspace-wide.
- [ ] Integration tests `expand_tests`, `torture_tests`, and
      `structural_tests` pass with the same test count as before.

## Out of scope

- Changes to DSL semantics, error messages, or warning content.
- Merging or renaming sibling phase modules (`composition`,
  `connection`, `scope`, `error`).
- Dropping the `Expander` struct entirely (tier 4 reduces it but
  retains the type).
- Touching `expand_pattern_def`, `flatten_song`, `index_songs`,
  or the connection primitives in `composition.rs` / `connection.rs`.

## Scheduling notes

Tiers are sequential. Tier 2 depends on tier 1's file layout; tier 3
depends on tier 2's cleanup; tier 3b depends on tier 3's extracted
functions; tier 4 depends on tier 3b's parameter threading. Running
tiers in parallel risks merge conflicts over expander internals.

Coordinate with any in-flight work that touches
`patches-dsl/src/expand/`. DSL grammar extensions (new statement
kinds, new param-entry forms) should rebase over whatever tier is
current rather than land against mixed tiers.
