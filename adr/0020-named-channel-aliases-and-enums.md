# ADR 0020 — Named channel aliases and enum declarations

**Date:** 2026-03-29
**Status:** Proposed

## Context

The DSL currently uses integer indices for multi-port modules and indexed
parameters. A `Sum(channels: 3)` module has ports `in[0]`, `in[1]`, `in[2]`
and parameters `gain[0]`, `gain[1]`, `gain[2]`. This is adequate but opaque:
the meaning of each channel is carried only in the patch author's head.

Practical use cases motivate named alternatives:

- **Mixer channels:** `channels: [drums, bass, guitar]` is clearer than
  `channels: 3` with comments.
- **Multi-output modules:** A `Split` or `Demux` with named outputs
  (`low`, `mid`, `high`) where per-channel parameters are grouped by name
  rather than scattered across indexed entries.
- **General readability:** Any module with indexed ports benefits from names
  that survive into connection syntax.

Separately, a patch-level vocabulary of named constants is useful for values
that appear in multiple places — MIDI CC numbers, channel indices, or any
integer that carries semantic meaning. The mechanism is general.

Both features are purely syntactic: aliases and enum values resolve to integers
(or strings) during Stage 2 expansion. `FlatPatch`, Stage 3, `ModuleGraph`,
and the audio engine are unaffected.

## Decision

### 1. Enum declarations

A new top-level declaration defines a named set of identifiers:

```text
enum <name> {
    <ident>,
    <ident>,
    ...
}
```

Each identifier maps to its zero-based position in the declaration order.
Enum values are integer constants: `kick` in `enum drum { kick, snare, hat }`
has value `0`, `snare` has value `1`, `hat` has value `2`.

Enum members are referenced with dot syntax: `drum.kick`. They may appear
anywhere a scalar value is accepted — in shape args, parameter values, arrow
scales, and port indices.

```text
enum drum {
    kick,
    snare,
    hat
}

patch {
    module mix : Sum(channels: 3) {
        gain[0]: 0.8,
        gain[1]: 0.6,
        gain[2]: 1.0
    }

    src_kick.out  -> mix.in[drum.kick]
    src_snare.out -> mix.in[drum.snare]
    src_hat.out   -> mix.in[drum.hat]
}
```

Enum declarations appear at file level, alongside templates and before the
patch block. Multiple enum declarations are permitted; names must be unique
across all enums. Whether member names must be globally unique or only unique
within their enum is a grammar decision deferred to implementation — global
uniqueness is simpler but more restrictive.

### 2. Alias lists in shape args

A shape arg that currently accepts an integer may alternatively accept a
bracketed list of identifiers:

```text
module mix : Sum(channels: [drums, bass, guitar])
```

This is equivalent to `Sum(channels: 3)` with the additional effect of
defining a **local alias map** scoped to that module instance:

```text
drums → 0
bass  → 1
guitar → 2
```

The alias list is syntactically an array of bare identifiers in shape-arg
position. The expander:

1. Counts the elements to derive the integer value for the shape arg.
2. Records the alias → index mapping, scoped to the module instance name.
3. Erases the list, emitting `channels: 3` in the `FlatModule`.

Alias lists are valid only in shape-arg position (the `(...)` block), not in
parameter values.

### 3. Alias-indexed port references

When a module has an alias map, port references may use alias names in index
position:

```text
mix.in[drums]    # resolves to mix.in[0]
mix.in[guitar]   # resolves to mix.in[2]
```

This uses the existing `port_index` grammar slot. There is no ambiguity with
template parameter references: template parameters use `<param>` syntax
(`mix.in[<n>]`), while aliases are bare identifiers (`mix.in[drums]`). The
grammar distinguishes the two forms lexically, so no precedence rule is needed.

A bare identifier in port-index position is resolved against the alias map of
the target module. If no alias matches, it is an error.

### 4. Alias-indexed parameter entries

Parameters may be indexed by alias name:

```text
module mix : Sum(channels: [drums, bass, guitar]) {
    gain[drums]:  0.8,
    gain[bass]:   0.6,
    gain[guitar]: 1.0
}
```

The expander resolves `gain[drums]` to `gain/0` in the flat representation,
identical to writing `gain[0]: 0.8`. This uses the existing `param_index`
grammar slot with the same resolution order as port indices (template param
first, then alias).

### 5. `@alias` parameter grouping blocks

For modules with multiple per-channel parameters, a grouping syntax avoids
repeating the alias on every entry:

```text
module eq : ThreeBandEQ(bands: [low, mid, high]) {
    @low: {
        freq:  200.0,
        gain:  -3.0,
        q:     0.7
    },
    @mid: {
        freq:  1000.0,
        gain:  1.5,
        q:     1.0
    },
    @high: {
        freq:  8000.0,
        gain:  -1.0,
        q:     0.7
    }
}
```

`@<index>: { key: value, ... }` is syntactic sugar. The index may be a raw
integer or an alias name:

```text
@0: { freq: 200.0, gain: -3.0, q: 0.7 }     # raw index
@low: { freq: 200.0, gain: -3.0, q: 0.7 }   # alias (resolves to the same thing)
```

Both desugar to:

```text
freq[0]: 200.0,
gain[0]: -3.0,
q[0]: 0.7
```

The `@` block does not introduce a new scope or namespace — it is purely a
grouping convenience. Nested `@` blocks are not supported.

When using an alias name, it must resolve against an alias map defined on the
enclosing module (from a shape-arg alias list). Using `@` with an
unresolvable name is an error. Raw integer indices are always valid.

### 6. Aliases and templates

Alias lists are a **call-site** feature, not a template-parameter feature.
Templates operate on integer arity via `<n>` and cannot receive or forward
alias lists.

A template defines its internal wiring using integer indices or arity
expansion — it does not know what names the caller might use:

```text
template MixBus(n: int, gains[n]: float = 1.0) {
    in:  ch[n]
    out: mix

    module sum : Sum(channels: <n>)
    sum.in[*n] <- $.ch[*n]
    $.mix      <- sum.out
}
```

The caller applies aliases at the instantiation site:

```text
module bus : MixBus(n: 3) {
    gains[drums]:  0.8,
    gains[bass]:   0.6,
    gains[guitar]: 1.0
}
```

But this requires the caller to know the arity and supply an alias map.
For this to work, the alias list is declared on the module instantiation's
shape args, not on the template's parameter list:

```text
module bus : MixBus(n: [drums, bass, guitar]) {
    gains[drums]:  0.8,
    gains[bass]:   0.6,
    gains[guitar]: 1.0
}
```

Here `n: [drums, bass, guitar]` resolves to `n: 3` (forwarded to the
template) and defines aliases `drums → 0`, `bass → 1`, `guitar → 2`
scoped to the `bus` module instance. The template body sees only `n = 3`
and expands arity accordingly. The aliases are used only in the call-site
parameter block and in connections referencing `bus` from outside.

Connections *inside* the template cannot use caller-supplied aliases — the
template is defined before any instantiation and has no knowledge of what
names a caller might choose. Internal wiring uses integer literals, `<param>`
references, or `[*n]` arity expansion.

### Grammar changes

```ebnf
# New top-level declaration
enum_decl     = "enum" ~ ident ~ "{" ~ (ident ~ ","?)* ~ "}"

# File gains enum declarations
file          = SOI ~ enum_decl* ~ template* ~ patch ~ EOI

# Shape args: scalar OR alias list
# Alias lists are valid at both primitive module and template instantiation
# sites. At a template call site, `n: [drums, bass, guitar]` resolves to
# n = 3 (forwarded to the template) plus a local alias map on the instance.
shape_arg     = ident ~ ":" ~ (alias_list | scalar)
alias_list    = "[" ~ (ident ~ ","?)* ~ "]"

# Parameter index: now accepts alias names (bare ident) alongside literals.
# Template param references use <param> syntax and are not ambiguous.
param_index   = { "[" ~ (param_index_arity | nat | ident) ~ "]" }

# Parameter block gains @-blocks
param_entry   = at_block | keyed_entry | shorthand
at_block      = "@" ~ (nat | ident) ~ ":" ~ table
keyed_entry   = ident ~ param_index? ~ ":" ~ value
shorthand     = param_ref

# Port index: bare ident is now an alias reference (not a template param).
# Template params use <param> syntax; arity expansion uses *ident.
# No change needed — the existing rule already accepts bare ident,
# but its semantics change from "template param lookup" to "alias lookup".
port_index    = ${ "[" ~ (port_index_arity | nat | ident) ~ "]" }

# Enum member references in scalar position
scalar        = ... | enum_ref
enum_ref      = ${ ident ~ "." ~ ident }
```

All other grammar rules are unchanged.

### Flat representation

All alias-related syntax is erased during Stage 2 expansion:

| Surface syntax | Flat representation |
|----------------|---------------------|
| `channels: [drums, bass, guitar]` | `channels: 3` (shape arg) |
| `mix.in[drums]` | `from_index: 0` / `to_index: 0` |
| `gain[drums]: 0.8` | `("gain/0", 0.8)` |
| `@low: { freq: 200.0, gain: -3.0 }` | `("freq/0", 200.0), ("gain/0", -3.0)` |
| `drum.kick` | `0` (integer literal) |

## Consequences

**FlatPatch, Stage 3, ModuleGraph, and the audio engine are unchanged.** All
alias resolution and enum substitution occur in Stage 2. The flat IR contains
only integer indices and concrete scalar values, as before.

**Aliases are module-scoped, not global.** Two modules may independently define
aliases `left` and `right` without conflict. The alias map is keyed by module
instance name and exists only during expansion.

**Enum values are file-scoped constants.** They provide a shared vocabulary for
values that appear in multiple places (e.g., MIDI CC numbers shared between
multiple modules, or channel indices used in both connections and parameters).
They are not types — the DSL performs no type checking on enum usage. Any
parameter accepting an integer will accept any enum member's integer value.

**No ambiguity between template parameters and aliases.** Template parameters
use `<param>` syntax; aliases are bare identifiers. The grammar distinguishes
the two lexically, so no shadowing or precedence rules are needed.

**Breaking change to bare-ident port index semantics.** The existing grammar
accepts `module.port[ident]` and interprets the bare identifier as a template
parameter reference (`PortIndex::Param`), resolved from the template's param
environment. This ADR changes that interpretation: a bare identifier in port
index position is now an alias lookup against the target module's alias map.
Template parameter references in index position must use `<param>` syntax
(`module.port[<n>]`), consistent with their syntax everywhere else. Any
existing patch source using `module.port[k]` to mean "index from template
param `k`" must be updated to `module.port[<k>]`. The same applies to
`param_index`: `gain[k]: value` (template param) becomes `gain[<k>]: value`.
This is a source-breaking change, but aligns index syntax with the `<param>`
convention established in ADR 0006's amendment, eliminating the last place
where bare identifiers were silently treated as parameter references.

**`@` blocks are sugar, not structure.** They do not appear in the AST as a
distinct node — the parser (or an early desugaring pass) expands them into
indexed parameter entries before the expander sees them. Alternatively, they
may be preserved in the AST and desugared during expansion; the choice is an
implementation detail with no semantic consequence.

## Alternatives considered

### Aliases as a parameter-block feature rather than shape-arg

Declaring aliases in the parameter block (`{ channels: [drums, bass, guitar] }`)
rather than the shape block. Rejected because channel count is a shape concern
(it determines port layout), and the alias list is syntactically a way of
specifying that count. Placing it in the shape block maintains the existing
distinction: shape determines structure, params determine behaviour.

### String-keyed parameters instead of integer indices

Rather than aliasing integers, use string keys natively throughout
(`gain["drums"]` in the flat IR). Rejected because the module descriptor and
`ParameterMap` are indexed by `(name, usize)` pairs, and all downstream
infrastructure (planner, parameter diffing, `ModulePool::update_parameters`)
operates on integer indices. Introducing string-keyed channels would require
changes across `patches-core`, `patches-engine`, and every module
implementation. Integer aliases achieve the same ergonomic benefit with zero
runtime impact.

### No `@` blocks — just alias-indexed params

Relying solely on `freq[low]: 200.0, gain[low]: -3.0, q[low]: 0.7` without
the `@` grouping syntax. This works but becomes unwieldy when a module has
many per-channel parameters (freq, gain, q, type, bypass). The `@` block
groups related settings visually and reduces repetition of the alias name.
The cost is one additional grammar rule; the benefit scales with the number
of per-channel parameters.

### Enum declarations with explicit integer values

Allowing `enum cc { cutoff = 74, resonance = 71 }` with explicit values
rather than implicit zero-based ordering. Deferred: the implicit scheme is
sufficient for the motivating use cases (named channel indices, shared
constants). Explicit values can be added later without breaking existing
patches, since the grammar extension is backward-compatible.
