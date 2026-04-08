# ADR 0006 — DSL surface syntax

## Status

Accepted

## Context

ADR 0005 defines the compilation pipeline for the patch DSL and the crate
structure that implements it. This ADR defines the *surface syntax* — the
language a patch author writes. It is a companion document to ADR 0005 and
should be read alongside it.

Requirements the syntax must satisfy:

- Module declarations with scalar, array, and table initialisation values
- Patch cable connections with optional scale factor
- Template definitions: named sub-patches with declared signal ports,
  instantiable as if they were primitive module types
- Port indexing for factory-configured multi-port modules (see ADR 0005)

## Decision

### Lexical conventions

```text
# Comment to end of line
Identifiers:  [a-zA-Z_][a-zA-Z0-9_-]*
Numbers:      integer or floating-point literals  (42, 440.0, -0.5)
Strings:      double-quoted  "hello"
Booleans:     true | false
```

### Init-param values

Module declarations carry a `{ key: value }` block of initialisation
parameters, evaluated once at graph-build time by the module's factory.

```ebnf
Value  = Scalar | Array | Table
Scalar = Number | String | Bool
Array  = "[" (Value ","?)* "]"
Table  = "{" (Ident ":" Value ","?)* "}"
```

Examples:

```text
{ frequency: 440.0 }
{ steps: [60, 62, 64, 65, 67, 69, 71, 72] }
{ pattern: [
    { note: 36, velocity: 1.0, gate: 0.5 },
    { note: 38, velocity: 0.7, gate: 0.5 },
] }
```

The `{ ... }` block is optional; omitting it is equivalent to an empty map.

### Module declarations

```text
module <name> : <TypeName>(<shape>) { <params> }
module <name> : <TypeName>(<shape>)              # params omitted
module <name> : <TypeName> { <params> }          # shape omitted (default)
module <name> : <TypeName>                       # both omitted
```

`<TypeName>` is resolved by `patches-interpreter` against the module factory
registry. `<name>` becomes the `NodeId` (or the namespace prefix for template
instances — see below).

The optional `(<shape>)` block passes named shape arguments to `Module::describe`,
which determines the module's port layout and parameter schema. The two current
shape fields are `channels` and `length`; both default to `0` when omitted.
Shape arguments are evaluated at graph-build time and are not modifiable by
parameter updates — a shape change forces a new module instance.

The optional `{ <params> }` block supplies the initial `ParameterMap`. Parameter
changes on hot-reload are effected by modifying this block and replanning;
no shape change occurs.

```text
module seq  : Seq(length: 16) { steps: [C2, B2, C3] }
module mix  : Sum(channels: 3)
module osc  : Osc { frequency: 440.0, waveform: sine }
module out  : AudioOut
```

### Port references

A port is addressed by its module name, port label, and optional index:

```text
<name>.<label>        # index 0 implied
<name>.<label>[k]     # explicit index k
```

For modules with a single port per label (the common case), the `[k]` suffix is
omitted. For factory-configured multi-port modules (e.g. `Sum(channels: 4)`)
the index selects among the factory-produced ports.

### Connections

Connections use bidirectional arrow syntax. Both arrows point in the direction
of signal flow; which to use is a matter of convention (see Templates below).

```text
<port-ref> ->          <port-ref>   # unscaled, left-to-right
<port-ref> -[<scale>]-> <port-ref>  # scaled,   left-to-right
<port-ref> <-          <port-ref>   # unscaled, right-to-left
<port-ref> <-[<scale>]- <port-ref>  # scaled,   right-to-left
```

`<scale>` is any finite `f32`, including negative values (phase inversion) and
values outside `[-1, 1]` (amplification). The scale is a property of the edge,
not of either port.

There is no poly annotation in the DSL. Cable kind (mono or poly) is inferred
from the port descriptors and validated at graph-build time; a kind mismatch is
an error.

### Scale composition at template boundaries

When a connection passes through a template port, the scales on the internal
and external edges compose by multiplication. This is handled by the expander
(Stage 2) when inlining the template: for each external edge touching a
template port, the expander finds the internal edge(s) at that port and
multiplies their scales.

```text
# Inside template:   $.audio    <-[0.5]- module.out
# External use:      v.audio    -[0.4]-> other.in
# Expanded result:   module.out -[0.2]-> other.in   (0.5 × 0.4)

# Inside template:   module.in  <-[0.5]- $.freq
# External use:      other.out  -[0.4]-> v.freq
# Expanded result:   other.out  -[0.2]-> module.in  (0.4 × 0.5)
```

This means a template port is a transparent wire with optional gain; scale
composition is commutative and associative, making the template boundary an
algebraic abstraction with no surprises.

### Template port fan-out

A template in-port may appear in multiple internal edges (fan-out). Each
external edge driving that port is multiplied by each internal edge's scale
independently, producing one expanded edge per internal target.

```text
# Inside template:
osc.freq  <-       $.freq    # internal scale 1.0
filt.freq <-[0.5]- $.freq    # internal scale 0.5

# External use:
other.out -[0.3]-> v.freq

# Expanded result:
other.out -[0.3]->  osc.freq
other.out -[0.15]-> filt.freq
```

Similarly, a template out-port may be driven by multiple internal edges
(multiple sources summed at the patch level — each becomes an independent
edge into the destination).

### Template definitions

A template is a named sub-patch with explicitly declared input and output
signal ports and an optional parameter interface.

```text
template <name>(<param-decls>) {
    in:  <port>, ...
    out: <port>, ...

    module ...
    ...
    <connection>
    ...
}
```

Template params are declared with a type and default value:

```text
template voice(attack: float = 0.01, decay: float = 0.1, sustain: float = 0.7) {
    in:  freq, gate, vel
    out: audio

    module osc  : Osc
    module env  : Adsr { attack: attack, decay: decay, sustain: sustain, release: 0.3 }
    module vca  : Vca

    osc.freq <- $.freq     # template in-port bindings  (convention: <-)
    env.gate <- $.gate
    env.vel  <- $.vel
    $.audio  <- vca.out    # template out-port binding  (convention: <-)

    osc.out -> vca.in      # internal connections       (convention: ->)
    env.out -> vca.cv
}
```

**Convention:**

- Use `<-` when writing a template port binding: `module.port <- template_port`
  and `template_out <- module.port`. The arrow points toward the destination,
  keeping the source expression on the right for future expression-language
  extensions.
- Use `->` for internal module-to-module connections.
- Both arrows are valid anywhere; the convention is not enforced by the parser.

**Template params** are declared at the `template` level with a type (`float`,
`int`, `bool`) and a default value. They appear as atoms in the param block of
internal module declarations. Template params are **not** forwarded to shape
args of internal modules; shape args are always literals, keeping the port
layout of internal modules statically determined.

A template is instantiated like a primitive module type:

```text
module v1 : voice(attack: 0.005, sustain: 0.6)   # decay uses default
module v2 : voice                                  # all defaults
```

During expansion (Stage 2 of the pipeline), internal `NodeId`s are namespaced:
`v1/osc`, `v1/env`, etc. Top-level edges referencing `v1.freq` are rewritten to
target the appropriate internal node and port, with scale composition applied.
Templates may be nested.

### Top-level patch block

All module declarations and connections at the root of the file must appear
inside a `patch { ... }` block. Template definitions appear outside it. Each
file contains exactly one `patch` block.

Declarations and connections within a `patch` or `template` body may appear in
any order; the expander performs a two-pass collection (declarations first,
then connection resolution).

```text
template voice(...) { ... }

patch {
    module clock  : Clock  { bpm: 120.0 }
    module seq    : Seq(length: 8) {
        steps: [60, 62, 64, 65, 67, 69, 71, 72]
    }
    module v1     : voice(attack: 0.01)
    module out    : AudioOut

    clock.semiquaver -> seq.clock
    seq.pitch        -> v1.freq
    seq.gate         -> v1.gate
    v1.audio         -> out.left
    v1.audio         -> out.right
}
```

### Grammar sketch (PEG notation)

```ebnf
File          = Template* Patch
Patch         = "patch" "{" Statement* "}"
Template      = "template" Ident ParamDecls? "{" PortDecls Statement* "}"
ParamDecls    = "(" (ParamDecl ","?)* ")"
ParamDecl     = Ident ":" TypeName ("=" Scalar)?
TypeName      = "float" | "int" | "bool"

PortDecls     = InDecl OutDecl
InDecl        = "in:"  CommaIdents
OutDecl       = "out:" CommaIdents
CommaIdents   = Ident ("," Ident)* ","?

Statement     = ModuleDecl | Connection
ModuleDecl    = "module" Ident ":" Ident ShapeBlock? ParamBlock?
ShapeBlock    = "(" (Ident ":" Scalar ","?)* ")"
ParamBlock    = "{" (Ident ":" Value ","?)* "}"

Value         = Table | Array | Scalar
Array         = "[" (Value ","?)* "]"
Table         = "{" (Ident ":" Value ","?)* "}"
Scalar        = Float | Int | Bool | String | Ident   # Ident = template param ref

Connection    = PortRef ForwardArrow PortRef
              | PortRef BackwardArrow PortRef
ForwardArrow  = "->"  | "-[" Number "]->"
BackwardArrow = "<-"  | "<-[" Number "]-"
PortRef       = ModuleIdent "." Ident Index?
ModuleIdent   = "$" | Ident        # $ refers to the enclosing template's port namespace
Index         = "[" Nat "]"
```

## Consequences

**Mono/poly is invisible to DSL authors.** Cable kind is declared by the module
implementation and validated at graph-build time. Authors simply connect ports;
a kind mismatch is a build error with a clear message.

**Scale is a property of the edge, not the port.** The `<-[N]-` and `-[N]->`
syntax makes this explicit. Scale composition at template boundaries is
algebraically transparent.

**Templates are a compile-time construct only.** There is no runtime notion of a
template instance. Hot-reload re-runs the full three-stage pipeline. Module
state is preserved via the `InstanceId` registry (ADR 0003); the `foo/osc`
namespacing scheme provides stable `NodeId`s as long as the patch source uses
stable module names.

**Template params are not forwarded to shape args.** Internal module port
layouts are always statically determined from literal shape args. This keeps
the expander simple and avoids dynamically variable port layouts.

**Expression language is a forward extension.** Template param references
currently appear only as scalar atoms in param blocks. A future expression
language (arithmetic, conditionals) would extend `Scalar` to `Expr` in the
grammar without changing the rest of the structure.

## Amendment — 2026-03-20: `<param>` syntax, unquoted strings, and structural interpolation

### Motivation

The original syntax used bare identifiers as template parameter references in
value positions (`attack: attack`). This is ambiguous once unquoted string
literals are desirable (`fm_type: log` instead of `fm_type: "log"`), and gives
no visual distinction between a param reference and a literal value. A
consistent, unambiguous syntax is needed for both.

### Decisions

**1. `<ident>` is the universal param-reference syntax.**

Wherever a template parameter value is to be substituted, it is written
`<param-name>`. This replaces the previous bare-`Ident`-as-param-ref convention.

```text
# Before
module env : Adsr { attack: attack, decay: decay, sustain: sustain, release: 0.3 }

# After
module env : Adsr { attack: <attack>, decay: <decay>, sustain: <sustain>, release: 0.3 }
```

`<ident>` may appear in:

- Value positions in param and shape blocks (replaces bare ident)
- Port label position in a port reference (`osc.<type>`)
- Arrow scale position (`-[<scale>]->`)

**2. Bare identifiers in value positions are unquoted string literals.**

A bare identifier appearing as a `Scalar` value is treated as a string, not a
param reference. This allows natural notation for enum-typed parameters:

```text
module osc : Osc { waveform: sine }       # 'sine' is a string
module filt : Filter { fm_type: log }     # 'log' is a string, not quoted
```

Quoted strings (`"sine"`) remain valid and equivalent. The convention is to
omit quotes for identifier-shaped strings.

**3. Shorthand param entry: `<param>` alone expands to `param: <param>`.**

Inside a `{ }` param block, a bare `<ident>` without a preceding key expands to
`ident: <ident>`. This is analogous to shorthand property syntax in modern
languages.

```text
# Verbose
module env : Adsr { attack: <attack>, decay: <decay>, sustain: <sustain>, release: 0.3 }

# Shorthand
module env : Adsr { <attack>, <decay>, <sustain>, release: 0.3 }
```

**4. Structural interpolation: port names and scale values.**

`<ident>` is valid in port-label position and in arrow scale position, enabling
templates parameterised on port selection and gain:

```text
template osc(type: str, scale: float) {
    in:  v_oct
    out: out

    module osc : Osc
    osc.v_oct      <- $.v_oct
    osc.<type> -[<scale>]-> $.out
}
```

At expansion time the expander resolves `<type>` to a string (used as the port
label) and `<scale>` to a float (used as the edge scale). Port-name validity is
checked in Stage 3 against the module descriptor, as with all port references.

### Updated grammar sketch

```ebnf
ParamDecl     = Ident ":" TypeName ("=" Scalar)?
TypeName      = "float" | "int" | "bool" | "str"

ParamBlock    = "{" (ParamEntry ","?)* "}"
ParamEntry    = Ident ":" Value        # explicit key: value
              | "<" Ident ">"          # shorthand: expands to ident: <ident>

Value         = Table | Array | Scalar
Scalar        = Float | Int | Bool | String | Ident | ParamRef
Ident         # bare identifier — unquoted string literal
ParamRef      = "<" Ident ">"          # template parameter reference

ForwardArrow  = "->" | "-[" ScalarExpr "]->"
BackwardArrow = "<-" | "<-[" ScalarExpr "]-"
ScalarExpr    = Number | ParamRef

PortRef       = ModuleIdent "." PortLabel Index?
PortLabel     = Ident | ParamRef       # literal label or param-interpolated label
```

All other grammar rules are unchanged.

### Consequences (amendment)

**Bare identifiers in value positions are no longer param references.** Any
existing patch source using `{ key: param_name }` as a shorthand for param
substitution must be updated to `{ key: <param_name> }` or the shorthand form
`{ <param_name> }`. The parser can produce a clear error ("bare identifier in
value position — did you mean `<param_name>`?") to guide migration.

**Enum values no longer require quoting.** `waveform: sine`, `mode: lfo`,
`fm_type: log` all work without double quotes. Quoted and unquoted forms are
interchangeable.

**Structural interpolation is resolved entirely in Stage 2.** Port name and
scale interpolations are substituted by the expander before the flat patch
reaches Stage 3. Stage 3 sees only concrete port labels and concrete scale
values.

## Alternatives considered

**Poly annotation on connections (`poly N`).** An earlier version of this ADR
proposed `source -> dest poly 16` syntax. Rejected: cable kind is a property
of port declarations, not connections; encoding it in the DSL would duplicate
information and create a source of inconsistency. Mono/poly is instead inferred
and validated at graph-build time.

**Postfix scale multiplier (`source -> dest * 0.4`).** Rejected in favour of
`-[N]->` because the scale is a property of the connection edge, not a
modifier on the destination port. The labelled-edge notation makes this
relationship explicit and generalises cleanly to template boundary composition.

**Template params forwarded to shape args.** Rejected because it would make
internal port layouts dynamically variable depending on the instantiation
params, complicating the expander and making port counts unpredictable at
template-definition time.
