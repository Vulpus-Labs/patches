# ADR 0070 — Stereo module sugar

## Status

Proposed (revised 2026-05-09: dropped `.l`/`.r` accessor and second
param block in favour of reusing existing `at_block` and `port[l]`
named-index forms; revised again 2026-05-09 to use an "always-insert"
splitter / joiner strategy that defers source-kind classification to
the planner's existing `CableKind::Mono → CableKind::Stereo` broadcast
rule — no descriptor lookup needed at desugar time, so the entire
expansion stays in `patches-dsl`)

## Context

The DSL has no concept of stereo at the module-declaration level. Stereo
audio is carried as two mono cables (left/right pair), with explicit
`StereoSplitter` / `StereoJoiner` utility modules at the boundary
([patches-modules/src/stereo_split.rs]). When a patch needs to apply a
mono-only DSP module to a stereo bus, the author writes the pair by hand:

```text
module split    : StereoSplitter
module crush_l  : Bitcrusher { depth: 8, rate: 0.8 }
module crush_r  : Bitcrusher { depth: 8, rate: 0.8 }
module join     : StereoJoiner

mix.out             -> split.in
split.out_left      -> crush_l.in
split.out_right     -> crush_r.in
crush_l.out         -> join.in_left
crush_r.out         -> join.in_right
join.out            -> out.in
```

This pattern recurs everywhere stereo audio crosses an effect chain. It is
verbose, error-prone (pair-name skew, parameter drift), and hostile to
quick experimentation. The alternative — implementing a stereo variant of
every module — multiplies the module surface for DSP that has no
intrinsically stereo behaviour, and pushes per-channel state into the
module rather than the graph.

A surface-syntax sugar that expands to the same hand-written pattern keeps
modules dumb (mono DSP only), keeps the runtime untouched, and removes the
drudgery from the patch source.

## Decision

Add a `stereo` keyword prefix to module declarations. The parser accepts
it; the expander rewrites a stereo module into a pair of mono instances
plus the connections needed to bridge stereo and mono signals at its
ports. The result is identical to the current hand-written form.

A `stereo module` behaves like a two-channel wrapper around an
**inherently single-channel** mono module — that is the only kind of
module type the sugar accepts. Wrapping a multi-channel module type
(e.g. `StereoMixer`, `Mixer(8)`) is a binding-stage error: the
multiplication concept already lives in those modules' shape arg.

### Surface syntax

```text
stereo module <name> : <TypeName>(<shape>) { <params> }
```

The single param block carries both shared params and per-channel
overrides — the `@l: { ... }` / `@r: { ... }` at_block form already
in use elsewhere (e.g. `StereoMixer` per-channel overrides) is reused
verbatim:

```text
stereo module crush : Bitcrusher {
    depth: 8
    @l: { rate: 0.8 }
    @r: { rate: 0.7 }
}
```

Top-level entries apply to both channels; `@l` / `@r` at_blocks override
the named keys on that channel only. Per-key index forms (`rate[l]: 0.8`)
are accepted equivalently — both reduce to the same per-side override.

### Channel selectors on ports

Ports on a stereo module are addressed two ways:

```text
<stereo-name>.<port>        # bus form — both channels at once
<stereo-name>.<port>[l]     # left mono instance only
<stereo-name>.<port>[r]     # right mono instance only
```

The selector reuses the existing `port[ident]` named-index form — no
new grammar surface. The expander interprets `[l]` / `[r]` as side
selectors when (and only when) the addressed module is a `stereo`
declaration; on plain mono modules `[l]` / `[r]` remain ordinary
named indices and resolve via the usual alias / param-arity machinery.

A bare `<stereo-name>.<port>` references the **stereo bus**: the pair
of ports treated as a single endpoint, expanded according to the
connection rules below. For an output port consumed in bus form, a
`StereoJoiner` is emitted; for input ports consumed in bus form, a
`StereoSplitter` (or a paired stereo source) feeds both sides.

### Connection rules

Let `S` denote a stereo source port (a stereo module's bus output, a
`StereoSplitter` joined back, or any source the type system labels stereo
in the future), and `M` denote a mono source. Targets are analogous.

| Source | Target | Expansion |
|--------|--------|-----------|
| M | mono port | direct edge (today's behaviour) |
| M | stereo bus | broadcast: two edges, one to each side |
| S | stereo bus | splitter inserted; L→l-side, R→r-side |
| S | mono port | **error.** No silent downmix. Use `Sum` or `port[l]`/`port[r]`. |
| M | `name.port[l]` / `name.port[r]` | direct edge to that side only |
| S | `name.port[l]` / `name.port[r]` | error. Pick a side from `S` first. |

Multiple connections into the same stereo-bus port sum, as today.
Mono sources broadcast then sum equally on both sides; stereo sources
contribute per side. Mono and stereo into the same port mix naturally:
the mono source adds equally to L and R, stereo adds per side.

### Splitter CSE

If two stereo-bus ports on stereo modules consume the **same** stereo
source, only one splitter is emitted; its `out_left` / `out_right` fan
to all consumers. Implementation: keep a `BTreeMap<SourcePort,
SplitterId>` during expansion, indexed by canonicalised source port.

### Joiner emission

A stereo module's bus output (`name.out`) is realised as a joiner only
when consumed *as a bus* — that is, with no `[l]` / `[r]` selector. If
every consumer reads `name.<port>[l]` or `name.<port>[r]`, no joiner is
emitted. Mixed consumption (some bus, some side-tap) emits one joiner
whose output fans to bus consumers; side-tap consumers read directly
from the underlying mono instance.

### Always-insert design

The bus-expansion rules below would naively need to know whether a
non-stereo-module port is mono or stereo (e.g. is `mix.out` mono or
stereo?). That information lives in the module descriptors, which
the DSL stage cannot consult. The way around it: **always insert a
`StereoSplitter` at a stereo module's bus input, regardless of source
kind.** The planner's existing `CableKind::Mono → CableKind::Stereo`
broadcast rule (`patches-planner::state::graph_index`) handles the
mono case transparently — a mono signal feeding the splitter's stereo
input gets duplicated to L/R, which the splitter then routes to the
two mono instances. A stereo signal feeds the splitter directly. Both
cases produce identical-to-hand-written output without the desugar
needing to know which is which.

Symmetrically: a stereo module's bus output is always wrapped in a
`StereoJoiner` when consumed in bus form. The joiner produces a stereo
cable; if the consumer expects mono, the existing connection validator
emits the canonical stereo→mono mismatch error.

The one optimisation worth keeping is **pair-direct** for the
stereo-module → stereo-module case: when both endpoints are stereo
modules with bus form, the underlying `__l` / `__r` instances on both
sides already exist, so we wire them directly and skip the join+split
sandwich. This is purely name-based (both endpoints are `is_stereo`
decls) — no descriptor info needed.

### Expansion algorithm

Run after parse, before tap and host-control desugars in
`patches-dsl::expand::expand`.

1. **Decl rewrite.** For each `stereo module X : T S { P }` split `P`
   into shared entries and `@l` / `@r` at_blocks (per-key index forms
   `key[l]` / `key[r]` are normalised into the corresponding at_block
   first), then emit two mono decls
   `X__l : T S { P_shared ⊕ P_l }`, `X__r : T S { P_shared ⊕ P_r }`.
   `⊕` is the existing param-merge for `at_block` overrides.
2. **Cable rewrite.** Walk all connections. For each endpoint where
   `name` is a stereo module:
   - `name.port[l]` → `name__l.port`, mark as side selector L.
   - `name.port[r]` → `name__r.port`, mark as side selector R.
   - `name.port` (no index) → mark as bus form.
3. **Bus expansion.** Classify each cable's `(source, target)`:
   - Both bus form on stereo modules: pair-direct — emit
     `(s__l.sp, t__l.tp)` and `(s__r.sp, t__r.tp)`. No splitter or
     joiner.
   - Plain → bus form on stereo module: ensure a `StereoSplitter` for
     the source `(module, port)` exists (CSE), feed `src.port → ~split.in`
     once, then emit `(~split.out_left, t__l.tp)` and
     `(~split.out_right, t__r.tp)`. The mono case is handled by the
     planner's mono→stereo broadcast at the splitter's input.
   - Bus form on stereo module → plain target: ensure a `StereoJoiner`
     for the source `(origin, port)` exists (CSE), feed it from
     `(s__l.sp, ~join.in_left)` and `(s__r.sp, ~join.in_right)` once,
     then emit `(~join.out, target)`.
   - Bus form → side selector: error (ADR rule — pick a side from
     the source first).
   - Side selector → side selector / plain: rewrite to underlying
     mono instance and emit unchanged.
   - Plain → plain: pass through.
4. **Joiner suppression.** Joiners are emitted lazily — only when a
   stereo module's bus output has at least one consumer in bus form.
   Side-tap-only consumption (every consumer reads `name.<port>[l]`
   or `name.<port>[r]`) emits no joiner; mixed bus + side-tap
   consumption emits one joiner whose output fans to bus consumers
   while side-tap consumers read the underlying mono instances.

The output is a flat patch validation-ready against module
descriptors. Stage 3 of the pipeline runs unchanged.

### Worked example

The drums patch [song1/drums.patches] currently ends with:

```text
module split : StereoSplitter
module out_crush_l : Bitcrusher { depth: 8, rate: 0.8 }
module out_crush_r : Bitcrusher { depth: 8, rate: 0.8 }
module join : StereoJoiner

rate_lfo.sine -[0.1]-> out_crush_l.rate_cv, out_crush_r.rate_cv
mix.out             -> split.in
split.out_left      -> out_crush_l.in
split.out_right     -> out_crush_r.in
out_crush_l.out     -> join.in_left
out_crush_r.out     -> join.in_right
join.out            -> out.in
```

Sugared:

```text
stereo module out_crush : Bitcrusher { depth: 8, rate: 0.8 }

rate_lfo.sine -[0.1]-> out_crush.rate_cv
mix.out               -> out_crush.in
out_crush.out         -> out.in
```

`mix.out` is the bus output of `StereoMixer`, hence stereo source ⇒
splitter inserted (CSE-shared if any other stereo module also reads
`mix.out`). `rate_lfo.sine` is mono ⇒ broadcast to both sides. `out.in`
consumes the stereo bus ⇒ joiner emitted.

## Pipeline placement

The entire stereo expansion is a syntactic pass in `patches-dsl`,
running alongside the existing tap-target and host-control desugars
before the template expander produces the `FlatPatch`. It does not
consult the module registry — the trick that makes this possible is
**always-insert** (see "Always-insert design" below).

The DSL stage handles:

- Decl rewrite: `stereo module X : T { ... }` → `X__l : T { ... }`
  and `X__r : T { ... }` with shared params merged with `@l` / `@r`
  per-side overrides.
- Selector rewrite: `X.<port>[l]` → `X__l.<port>` (and same for r).
- Bus expansion: bus-form cables get a `StereoSplitter` (input side)
  or `StereoJoiner` (output side) inserted, with CSE per source.
- Pair-direct optimisation: when both endpoints are stereo modules
  in bus form, wire the underlying mono instances directly without an
  intervening join+split sandwich.
- Identifier-clash check: rejects user names that collide with the
  synthesised `__l` / `__r` suffix.
- Heuristic single-channel-type check: rejects
  `stereo module x : T(channels: N)` when N > 1.

After this pass `FlatPatch` is descriptor-validation-ready in the
same shape it would be for a hand-written stereo patch — the binder
needs no stereo-specific awareness.

## Tooling implications

### Pest grammar

Exactly one change in `patches-dsl/src/grammar.pest`:

1. `module_decl` gains an optional `stereo` keyword prefix with
   word-boundary lookahead (identical to `bool_lit`'s treatment) so
   identifiers like `stereo_in` are not consumed.

`@l` / `@r` at_blocks reuse the existing rule — already valid wherever
a `param_block` is. `port[l]` / `port[r]` reuse the existing
`port_index` named-form. No grammar work for either.

### Tree-sitter grammar parity

`patches-lsp/tree-sitter-patches/grammar.js` mirrors pest: the single
new keyword. The corpus driver (`patches-lsp/src/syntax_corpus.rs`)
enforces parity, so a `stereo_module.corpus` entry under
`patches-lsp/tests/syntax_corpus/` exercises both parsers against
representative inputs.

### Highlights

`patches-lsp/tree-sitter-patches/queries/highlights.scm` highlights the
new `stereo` keyword.

### LSP intelligence

The LSP currently builds an AST per-file
(`patches-lsp/src/ast_builder/`), runs expansion via
`patches-lsp/src/expansion.rs`, and serves completions, hover, inlay,
and navigation from the resolved graph. The sugar lives in the
desugar/expand pipeline used by the LSP, so:

- **Hover** on `stereo module crush` shows the underlying module
  descriptor with a `(stereo-paired)` annotation.
- **Hover** on `crush.<port>[l]` or `crush.<port>[r]` resolves through
  to the underlying mono instance's port descriptor; bus-form hover
  shows the descriptor without a side annotation.
- **Completion** on `crush.<port>[` offers `l` and `r` when the
  enclosing module is a `stereo` decl.
- **Inlay hints** can show the implicit splitter/joiner topology when a
  user-facing setting is enabled, but default off (the sugar's whole
  point is to hide it).
- **Go-to-definition** on a stereo module reference resolves to the
  `stereo module` decl site. The `[l]` / `[r]` selector is a syntactic
  locator, not a separate decl.
- **Diagnostics** for stereo-into-mono surface at the cable site, not
  the decl site, with a fix-it suggesting `Sum` or a `port[l]` /
  `port[r]` selector.

### vscode plugin

`patches-vscode/syntaxes/` TextMate grammar gains a `stereo` keyword
match. No other vscode-specific changes — completions and diagnostics
flow from the LSP.

## Consequences

**Modules stay mono.** No new module trait, no per-channel state
expansion in DSP code. The runtime sees only the existing mono modules
and stereo splitters/joiners.

**Sugar is a desugar-time concept only.** No runtime artefact, no
descriptor changes. Hot-reload, planner, observation, all unaffected.

**Per-channel state is duplicated, not shared.** Each side gets its
own module instance with its own state buffers. This is the same
behaviour as the hand-written pattern; no change.

**Stereo→mono is an explicit error, not a silent downmix.** This may
catch users by surprise, but matches the mono/poly cable-kind discipline
already in force (ADR 0006). The error message includes the two escape
hatches (`Sum`, `port[l]` / `port[r]`).

**Wrapped module type must be single-channel.** `stereo module x : T(N)`
where `T(N)` is a multi-channel module is rejected at binding time.
The sugar's contract is "this is a mono DSP I want applied per stereo
channel" — multiplying that against an already-multi-channel type
produces an N×2 grid that has no single sensible interpretation.

**Naming convention `__l` / `__r` is internal.** The user never sees it.
If it collides with a user identifier (e.g. someone literally named a
module `crush__l`), the expander emits a clash diagnostic; double
underscore is unconventional enough that the false-positive risk is low.

**Future stereo-aware ports.** Some modules may eventually declare ports
as intrinsically stereo (e.g. a panner whose output is a stereo pair).
The connection rules above are written in terms of "stereo source" and
"stereo target", anticipating a future `CableKind::Stereo` propagation
through the descriptor. For now, the only stereo sources/targets are
stereo modules' bus ports and the existing splitter/joiner instances.

## Alternatives considered

**Auto-stereoize at runtime.** Module declares `process_mono`; runtime
runs it twice when fed stereo. Rejected: hidden control flow, harder to
reason about CPU and per-instance state, complicates the planner.

**Stereo trait on modules.** Each module opts in by implementing a
stereo path. Rejected: pushes the multiplication into DSP code; modules
with no intrinsic stereo behaviour pay implementation cost for the
sugar's benefit.

**Bus-typed connections without sugar.** Type connections by channel
count and propagate. Useful as a future direction but does not fix the
verbosity problem at the decl site — the user still writes a pair of
modules. Compatible with this ADR; stereo-typed connections would slot
into the connection rules as a new "stereo source" generator.

**`x2` suffix or array sugar (`Bitcrusher x 2`).** Generic vector sugar
for any arity. Rejected: stereo is a specific endpoint with established
splitter/joiner conventions; generalising to N-channel adds rules
(routing matrix, channel labels) that are not needed for the immediate
problem.
