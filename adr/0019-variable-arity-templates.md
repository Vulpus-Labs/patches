# ADR 0019 — Variable-arity template ports and parameters

**Date:** 2026-03-20
**Status:** Accepted

## Context

ADR 0006 defined the surface syntax for templates and explicitly excluded
variable-arity port groups, stating "template params are not forwarded to shape
args" and that internal port layouts are always statically determined. That
constraint was conservative: at the time the expander was being designed, the
added complexity of arity expansion was not worth the benefit.

Since then, practical template authoring has surfaced a clear need: a
`LimitedMixer` template (or any N-channel voice bus) wants to expose N in-ports
and N parameter groups, where N is a template argument. Writing this without
arity support requires either a fixed-size template per channel count or manually
indexing every port.

The key insight that makes this tractable: template parameters are fully resolved
to concrete `Scalar` values by the time the expander processes a body. The
expander therefore has all the information it needs to expand arity groups into
concrete, individually-indexed connections. `FlatPatch`, Stage 3 (graph builder),
`ModuleGraph`, and the audio engine are entirely unaffected.

## Decision

### Port index syntax — three forms

Port references in connections gain two new index forms alongside the existing
literal index:

```text
module.port       # literal index 0 (unchanged)
module.port[0]    # literal index 0 (unchanged)
module.port[k]    # param index: single port at index param k
module.port[*n]   # arity expansion: expand over 0..n
```

The bracket content determines the form:
- An integer literal → literal index (existing behaviour).
- A bare identifier → param index: the identifier is looked up in the current
  param environment and its integer value used as the port index. Results in
  exactly one connection.
- An identifier preceded by `*` → arity expansion: the identifier is looked up
  to obtain N, and the connection is emitted N times, once for each index in
  `0..N`.

The `*` sigil only appears in connection contexts (use sites). It is not valid
in declarations.

### Template port declarations — named arity

Template `in:` and `out:` port declarations may carry a named-arity annotation:

```text
in:  freq, gate          # single ports (unchanged)
in:  audio[n]            # n ports named 'audio', where n is a template param
out: left, right, bus[n] # mix of single and group ports
```

The `[param]` suffix in a declaration binds the arity to a named template
parameter. No `*` appears in declarations; the brackets here are a
declarative annotation, not a spread operator.

### Template parameter groups

Template parameter declarations may also be grouped with the same arity syntax:

```text
template LimitedMixer(size: int, level[size]: float = 1.0, pan[size]: float = 0.0) {
    in:  in[size]
    out: left, right
    ...
}
```

`level[size]: float = 1.0` declares `size` float parameters named `level`,
defaulting to 1.0. At a call site, group params may be supplied in three forms:

```text
# Broadcast: scalar applied to all slots
module lm : LimitedMixer(size: 4, level: 0.8)

# Explicit array: must have exactly size elements
module lm : LimitedMixer(size: 4, level: [0.8, 0.9, 0.7, 1.0])

# Per-index: individual assignments; unset slots use the declared default
module lm : LimitedMixer(size: 4, level[0]: 0.8, level[1]: 0.3)
```

Per-index assignment uses the existing `name[k]` literal-index param syntax
(already present in ADR 0006 / T-0132); group params are not a special case in
the flat IR. The expander distributes group params to `level/0`, `level/1`, …
before passing them to Stage 3, which already handles indexed parameter keys.

### Expander handling (Stage 2)

When the expander encounters a connection involving `[*n]`:

1. Look up `n` in the current `param_env`. If absent or not an integer, emit an
   error with span.
2. For `i` in `0..n`, substitute the wildcard index with `i` and emit one
   `FlatConnection` with `from_index`/`to_index` set to `i`.

Template boundary rewiring (`$.port[*n]`) follows the same rule: the in-port
map registers `n` entries (`port/0` through `port/n-1`) during body expansion.
Each caller-side `instance.port[*n]` then fans into the appropriate entry.

Arity expansion composes with scale multiplication at template boundaries
exactly as scalar connections do: each expanded connection is an independent
edge and receives the composed scale independently.

### Relationship to `Scalar::Ident` substitution in shape args

`Scalar::Ident` substitution in shape args (e.g. `Mixer(channels: <size>)`) is
already implemented. This ADR builds on that: `<size>` resolves to a concrete
integer before Stage 3, so `Mixer` is built with a concrete channel count.
The `[*size]` arity expansion in connections then mirrors that count by emitting
exactly `size` concrete connections. The two mechanisms are complementary and
must be kept consistent by the patch author; a mismatch (e.g. `channels: 4`
but `in[*3]`) is caught at Stage 3 when port indices are validated against the
module descriptor.

## Consequences

**`FlatPatch`, Stage 3, `ModuleGraph`, and the audio engine are unchanged.**
All arity expansion is resolved in Stage 2. After expansion, the flat patch
contains only concrete `(module_id, port_name, port_index)` triples — the same
form it has always carried.

**ADR 0006's constraint "template params are not forwarded to shape args" is
superseded.** `Scalar::Ident` substitution in shape args was already
implemented; this ADR formalises that it is intentional and extends the same
mechanism to port index expressions and arity expansion.

**Multiple independent arity groups are supported.** A template may declare
`in[n]` and `out[m]` with different params; `[*n]` and `[*m]` expand
independently. The bracket always names its own arity source explicitly.

**Hot-reload stability is unaffected.** Arity is part of the template parameter
interface. Changing `size` forces new module instances only if the underlying
module's shape args change (which they do for `Mixer(channels: <size>)`); the
`InstanceId`-based registry handles this the same way as any other shape change.

## Alternatives considered

**`*` suffix on port names (`in*`, `mixer.in*`).** Requires an implicit
convention to determine which param drives the arity (e.g. the first `int`
param, or a param conventionally named `size`). Rejected in favour of the
explicit `[n]` / `[*n]` syntax, which names the arity source at every use site
and supports multiple independent arity groups without ambiguity.

**Keeping the original exclusion of variable arity.** Workable for small
templates but forces either combinatorial fixed-size variants or verbose manual
indexing for any N-channel construct. The expander complexity is contained;
the user-facing benefit is significant.

**Resolving arity in Stage 3 (graph builder).** Would require `FlatPatch` to
carry unresolved wildcard nodes, complicating the flat IR and the graph builder
without benefit. Stage 2 has all the information needed (param values are
concrete after substitution); Stage 3 should remain ignorant of template
concepts.
