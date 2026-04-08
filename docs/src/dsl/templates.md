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
| `string` | `waveform: string = "sine"` |
| `bool` | `enabled: bool = true` |

Parameters without a default are required at every call site.
