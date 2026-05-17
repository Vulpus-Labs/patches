# Templates and abstraction

A *template* is a named, parameterised subgraph that you can stamp
out many times in a patch. Templates are expanded at compile time —
they are a DSL-level abstraction, not a runtime construct. Every
instantiation produces concrete module instances with mangled names,
and the result is what the engine actually runs.

Use templates to:

- Define a synth voice once and instantiate it per pitch / role.
- Wrap a stable subgraph (an effect chain, a stereo widener, a
  drum kit channel) so the top-level patch reads at a higher
  level of abstraction.
- Parameterise across patches — a `voice` template lives in one
  place and a hundred patches can use it.

## Defining a template

A template definition is a top-level block:

```patches
template voice(attack: float = 0.01, decay: float = 0.1,
               sustain: float = 0.7, release: float = 0.3) {
    in:  voct, gate, trigger
    out: audio

    module osc : Osc
    module env : Adsr { attack: <attack>, decay: <decay>,
                        sustain: <sustain>, release: <release> }
    module vca : Vca

    $.voct    -> osc.voct
    $.gate    -> env.gate
    $.trigger -> env.trigger

    osc.sawtooth -> vca.in
    env.out      -> vca.cv
    vca.out      -> $.audio
}
```

Three things to notice:

1. **Parameter list** (`attack: float = 0.01, ...`). Typed, with
   optional defaults. Types are `float`, `int`, `bool`, `str`,
   `pattern`, `song`. Parameters without a default are required at
   the call site.
2. **Boundary ports** (`in:` and `out:` lists). These are the
   template's external interface. Inside the body, `$.name`
   refers to the boundary port `name`.
3. **Parameter references** (`<attack>`). Substituted wherever a
   value is expected in the body: parameter values, scale factors,
   shape arguments. The substitution happens during expansion, so
   `attack: <attack>` becomes `attack: 0.01` in the expanded copy.

The body is otherwise just patch syntax — modules, connections,
scaled cables. Templates can also use the `<-` backward arrow,
which makes destination-first writing cleaner for `out:` ports:

```patches
$.audio <- vca.out
```

is equivalent to `vca.out -> $.audio`.

## Instantiating

A template instantiates with the same `module name : Type` syntax as
a built-in module, except the type is the template name and the
parentheses carry the template arguments:

```patches
patch {
    module bass_v  : voice(attack: 0.005, decay: 0.15, sustain: 0.6)
    module lead1_v : voice(attack: 0.02,  decay: 0.3,  sustain: 0.5)
    module lead2_v : voice    # all defaults
    module mix     : StereoMixer([bass, lead1, lead2])

    bass_v.audio  -> mix.in[bass]
    lead1_v.audio -> mix.in[lead1]
    lead2_v.audio -> mix.in[lead2]
}
```

Each instantiation expands to its own copy of the template's
modules; the engine sees `bass_v_osc`, `bass_v_env`, `bass_v_vca`,
`lead1_v_osc`, and so on as distinct instances. From outside, an
instance presents only its declared boundary ports — `bass_v.voct`,
`bass_v.gate`, `bass_v.audio`. The internals are invisible.

### Parameter types

| Type      | Accepts                                          |
| --------- | ------------------------------------------------ |
| `float`   | Numeric values; `int` coerces to `float`.        |
| `int`     | Integer values only.                             |
| `bool`    | `true` / `false`.                                |
| `str`     | Quoted string or bare identifier.                |
| `pattern` | A pattern name (for tracker-driven templates).   |
| `song`    | A song name.                                     |

Type checking is strict — a `float` parameter rejects `true`; a
`bool` rejects `0`. The error message names the parameter and the
expected type.

## Arity parameters

A template can take a parameter that controls the number of ports.
Inside the body, `[*name]` expands per-index:

```patches
template summing_voice(size: int, level[size]: float = 1.0) {
    in:  audio[size]
    out: mixed

    module sum : Sum(<size>)

    $.audio[*size] -> sum.in[*size]
    sum.out        -> $.mixed
}
```

Instantiate it with a concrete size, and the connection
`$.audio[*size] -> sum.in[*size]` expands into one cable per index:
`$.audio[0] -> sum.in[0]`, `$.audio[1] -> sum.in[1]`, and so on.
The `level[size]` indexed parameter accepts per-index values:
`level[0]: 0.8, level[1]: 0.6, ...`.

## Scale composition across boundaries

When a scaled connection crosses a template boundary, the scales
multiply. A patch-level `-[0.5]->` into a template that internally
attenuates with `-[0.3]->` produces an effective scale of `0.15` at
the underlying cable — there is no intermediate buffer.

```patches
# Inside the template:
module mix : Sum(2)
$.in -[0.3]-> mix.in[0]
...

# In the patch:
src.out -[0.5]-> tmpl.in
# Effective scale at sum.in[0]: 0.5 * 0.3 = 0.15
```

This means template authors don't need to leave headroom for
"unknown caller scaling" — the multiplication composes
predictably.

## Nesting

Templates can instantiate other templates. The expander flattens
the whole tree before the patch is built:

```patches
template filtered_voice(...) {
    in: voct, gate, trigger
    out: audio

    module v : voice(attack: 0.005, ...)
    module f : Lowpass { cutoff: 1.2kHz }

    $.voct    -> v.voct
    $.gate    -> v.gate
    $.trigger -> v.trigger
    v.audio   -> f.in
    f.out     -> $.audio
}
```

`patch { module fv : filtered_voice; ... }` expands to a `v_voice`
sub-instance, a `v_voice_osc`, `v_voice_env`, `v_voice_vca`, and a
`v_filter` — six concrete instances from one declaration. Recursion
is bounded by depth; infinite recursion is caught at expansion time
with a clear error.

## When to use templates

Use them when:

- The same subgraph appears three or more times.
- A subgraph has a stable interface (a few clearly-named boundary
  ports) and you want to vary internal parameters.
- You want to name an abstraction (`voice`, `kit_channel`, `widener`)
  that makes the top-level patch read at a higher level.

Skip them when:

- A subgraph appears once and won't be reused. Inline modules read
  better than a one-shot template.
- You're tempted to use templates to *abstract over module types*.
  Templates instantiate a fixed set of modules with parameterised
  values — they don't let you swap `Osc` for `PolyOsc`. For that,
  write two templates.

## Templates as facades

A useful mental model: a template is a *typed facade* over the
underlying graph. The parameter list declares which knobs are
exposed; the port list declares the connection surface. The body —
which modules get instantiated and how they're wired — is hidden
implementation. Patches that use the template see only the typed
interface; the expander erases the abstraction before the engine
ever runs.

This is why scale composition, port-arity expansion, and parameter
substitution all happen at expansion time: templates are a compile
step, not a runtime layer.
