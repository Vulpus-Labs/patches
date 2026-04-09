# DSL reference

This is the complete syntax reference for `.patches` files. It assumes familiarity with the concepts in [Building a patch](building-a-patch.md).

## File structure

A patch file contains zero or more template definitions followed by exactly one `patch` block. Comments run from `#` to end of line. Whitespace is insignificant.

```patches
# optional templates
template voice(...) {
    ...
}

# required: exactly one patch block
patch {
    ...
}
```

## Module declarations

```
module <name> : <Type>
module <name> : <Type> { <params> }
module <name> : <Type>(<shape-args>) { <params> }
```

- **name** — an identifier (`[a-zA-Z_][a-zA-Z0-9_-]*`). Used for connections and for identity matching across hot-reloads.
- **Type** — the module type name as registered in the module registry (e.g. `Osc`, `PolyAdsr`, `Sum`).
- **shape-args** — key-value pairs that control port arity. Written in parentheses before the parameter braces: `Sum(channels: 3)`.
- **params** — key-value pairs configuring the module's parameters.

### Parameter syntax

Parameters are comma-separated `key: value` pairs:

```patches
module env : Adsr { attack: 0.01, decay: 0.1, sustain: 0.8, release: 0.3 }
```

Indexed parameters use bracket notation:

```patches
module mix : StereoMixer(channels: 4) { level[0]: 0.8, pan[0]: -0.5 }
```

At-block syntax groups parameters by index:

```patches
module dly : Delay(channels: 2) {
    @0: { delay_ms: 250, feedback: 0.4 },
    @1: { delay_ms: 375, feedback: 0.3 }
}
```

When the module shape defines named aliases, those can be used instead of numeric indices:

```patches
module mix : Mixer(channels: [drums, bass, lead]) {
    @drums: { level: 0.8 },
    @bass:  { level: 0.6 }
}
```

### Parameter value types

| Syntax | Type | Notes |
|--------|------|-------|
| `440.0` | float | bare decimal |
| `2` | int | bare integer |
| `440Hz` | frequency | converted to V/oct; also `2.5kHz` |
| `-6dB` | amplitude | converted to linear (0 dB = 1.0, -6 dB ≈ 0.5) |
| `C4`, `A#3`, `Bb2` | note name | converted to V/oct |
| `true` / `false` | boolean | |
| `linear` | string | unquoted; `"linear"` also works |
| `"hello world"` | string | quotes required if value contains spaces |
| `[1, 2, 3]` | array | |

Frequency, note, and dB literals are case-insensitive. There is no time-unit suffix; durations are bare floats in seconds.

## Connections

### Forward connection

```patches
osc.sine -> out.in_left
```

### Scaled forward connection

```patches
osc.sine -[0.5]-> out.in_left
```

The scale factor is a float multiplied onto the signal at the cable level.

### Backward connection

```patches
$.audio <- vca.out
$.audio <-[0.5]- vca.out
```

`<-` connects from right to left. Primarily useful inside templates for wiring boundary ports. The scaled form uses `<-[scale]-`.

### Indexed ports

```patches
osc_a.sine -> mix.in[0]
osc_b.sine -> mix.in[1]
```

The index in brackets selects which slot of a multi-port to connect.

### Arity expansion

```patches
$.audio[*size] -> sum.in[*size]
```

The `[*name]` wildcard expands into one connection per index from 0 to `name - 1`. Only valid inside templates where `name` is an int parameter.

### Rules

- Each input port accepts exactly one cable. A second connection to the same input is an error.
- An output port can drive any number of inputs.
- Mono outputs can only connect to mono inputs; poly to poly. Use `MonoToPoly` / `PolyToMono` to bridge.

## Templates

Templates define reusable sub-graphs that are expanded at compile time with no runtime cost.

### Definition

```patches
template voice(attack: float = 0.01, decay: float = 0.1, sustain: float = 0.7) {
    in:  voct, gate
    out: audio

    module osc : Osc
    module env : Adsr { attack: <attack>, decay: <decay>, sustain: <sustain>, release: 0.3 }
    module vca : Vca

    $.voct    -> osc.voct
    $.gate    -> env.gate
    osc.sine  -> vca.in
    env.out   -> vca.cv
    $.audio   <- vca.out
}
```

- **Parameters** are typed: `float`, `int`, `str`, `bool`. Parameters with `= default` are optional at the call site; those without are required.
- **Boundary ports** are declared with `in:` and `out:` lists. Inside the template body, `$.name` refers to a boundary port.
- **Parameter references** `<name>` substitute the parameter's value anywhere in the template body: in parameter values (`{ attack: <attack> }`), port names (`osc.<wave>`), and cable scales (`-[<gain>]->`).

### Instantiation

```patches
patch {
    module v1 : voice(attack: 0.005, sustain: 0.6)
    module v2 : voice    # all defaults

    v1.audio -> out.in_left
    v2.audio -> out.in_right
}
```

Each instantiation expands to a separate copy of the template's modules with mangled names to avoid collisions.

### Parameter types

| Type | Accepts |
|------|---------|
| `float` | numeric values (int-to-float coercion allowed) |
| `int` | integer values |
| `str` | string values |
| `bool` | `true` / `false` |

Type checking is strict. A `float` parameter rejects `true`; a `bool` rejects `0`.

### Arity parameters

A template parameter can control the number of ports:

```patches
template mixer(size: int, level[size]: float = 1.0) {
    in: audio[size]
    out: mixed

    module sum : Sum(channels: <size>)

    $.audio[*size] -> sum.in[*size]
    $.mixed        <- sum.out
}
```

The `[*size]` expansion generates one connection per index, scaling automatically with the arity.

### Scale composition

When a scaled connection crosses a template boundary, the scales are multiplied. A connection `-[0.5]->` into a template that internally has `-[0.3]->` produces an effective scale of 0.15 at the underlying cable.

### Nesting

Templates can instantiate other templates. A `filtered_voice` template can contain `module v : voice(...)` and add further processing. Expansion is recursive — the outermost template is fully flattened before the patch is built.
