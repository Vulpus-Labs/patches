# Templates

Templates let you define reusable sub-graphs with typed parameters and boundary
ports. They are expanded at compile time — there is no runtime cost for using a
template versus writing the modules inline.

## Defining a template

```patches
template voice(freq: float = 440Hz) {
    in: gate, trigger, voct
    out: audio

    module osc : Osc { frequency: <freq> }
    module env : Adsr { attack: 0.01, decay: 0.1, sustain: 0.8, release: 0.3 }
    module vca : Vca

    $.gate    -> env.gate
    $.trigger -> env.trigger
    $.voct    -> osc.voct
    osc.sine  -> vca.in
    env.out   -> vca.cv
    $.audio   <- vca.out
}
```

The `freq` parameter default (`440Hz`) is converted to a V/oct value at parse
time. If you pass a bare float it is treated as V/oct directly — not Hz.

### Boundary ports

`$.audio <- vca.out` declares `audio` as an output boundary port of the template.
When the template is used in a patch, `voice.audio` refers to this port.

Input boundary ports (`in: gate, trigger, voct`) work the same way in reverse:
`$.gate -> env.gate` routes the external signal into the template.

Boundary ports can be inputs, outputs, or both:

```patches
template filtered(cutoff: float = 1000Hz) {
    in: input
    out: output

    module filt : Lowpass { cutoff: <cutoff> }

    $.input  -> filt.in
    $.output <- filt.out
}
```

## Using a template

```patches
patch {
    module kbd : MidiIn
    module v1  : voice
    module out : AudioOut

    kbd.gate    -> v1.gate
    kbd.trigger -> v1.trigger
    kbd.voct    -> v1.voct
    v1.audio    -> out.in_left
    v1.audio    -> out.in_right
}
```

Each `module v : voice(...)` instantiation expands to a separate copy of the
template's modules, with names mangled to avoid collisions.

## Parameter types

| Type | Example |
| --- | --- |
| `float` | `level: float = 0.5` |
| `int` | `voices: int = 4` |
| `str` | `waveform: str = "sine"` |
| `bool` | `enabled: bool = true` |

Parameters without a default are required at every call site.

Type checking is strict: a `float` parameter only accepts numeric values
(int-to-float coercion is allowed), a `bool` only accepts `true`/`false`,
and a `str` only accepts string values.

## Parameter references

Inside a template body, `<name>` substitutes the parameter's value:

```patches
template voice(freq: float = 440Hz, wave: str = "sine") {
    in: gate, trigger, voct
    out: audio

    module osc : Osc { frequency: <freq> }
    module env : Adsr
    module vca : Vca

    $.gate    -> env.gate
    $.trigger -> env.trigger
    $.voct    -> osc.voct
    osc.<wave> -> vca.in
    env.out    -> vca.cv
    $.audio    <- vca.out
}
```

Parameter references can appear in:

- parameter values: `{ frequency: <freq> }`
- port labels: `osc.<wave>` (the `str` parameter's value becomes the
  port name)
- cable scales: `-[<gain>]->`
- shorthand form: `{ <freq> }` desugars to `{ freq: <freq> }`

## Arity parameters

Template parameters and ports can be parameterised by arity. An arity
variable controls the number of ports or indexed parameters:

```patches
template mixer(size: int, level[size]: float = 1.0) {
    in: audio[size]
    out: mixed

    module sum : Sum(channels: <size>)

    $.audio[*size] -> sum.in[*size]
    $.mixed        <- sum.out
}
```

The `[*size]` wildcard expands into one connection per index
(0, 1, …, size−1). This is equivalent to writing each connection
individually but scales automatically with the arity.
