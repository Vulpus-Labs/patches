# Patch syntax overview

A patch file contains zero or more template definitions followed by exactly one
`patch` block.

```patches
# Comments begin with #

template voice(freq: float = 440Hz) {
    in: gate, trigger, voct
    out: audio
    # ... module declarations and connections ...
}

patch {
    # ... module declarations and connections ...
}
```

## Module declarations

```patches
module <name> : <Type>
module <name> : <Type> { <param>: <value>, ... }
module <name> : <Type>(<arity-param>: <n>) { <param>: <value>, ... }
```

Examples:

```patches
module osc : Osc { frequency: 440Hz }
module env : Adsr { attack: 0.01, decay: 0.1, sustain: 0.8, release: 0.3 }
module mix : StereoMixer(channels: 4) { level[0]: 0.8, pan[0]: -0.5 }
```

## Connections

```patches
<from-module>.<output-port> -> <to-module>.<input-port>
<from-module>.<output-port> -[<scale>]-> <to-module>.<input-port>
```

The scale is a floating-point multiplier applied to the signal on that cable.
Negative scales invert the signal.

## Parameter values

| Syntax | Meaning |
| --- | --- |
| `440.0` | bare float |
| `2` | bare integer |
| `440Hz` / `2.5kHz` | frequency — converted to V/oct internally; case-insensitive |
| `-6dB` | amplitude in decibels — converted to linear (0 dB = 1.0, −6 dB ≈ 0.5); case-insensitive |
| `C4` / `A#3` / `Bb2` | note name — converted to V/oct; case-insensitive |
| `linear` | unquoted string (quotes optional: `"linear"` also works) |
| `"hello world"` | quoted string (required if the value contains spaces) |
| `true` / `false` | boolean |
| `[1, 2, 3]` | array of values |
| `{ x: 1, y: 2 }` | table (key-value map) |

There is no time-suffix literal. Duration parameters (e.g. attack, release)
take bare floats representing seconds.

## At-block syntax

Indexed parameters can be grouped per index using `@` blocks instead of
repeating the index on each key:

```patches
module dly : Delay(channels: 2) {
    @0: { delay_ms: 250, feedback: 0.4 },
    @1: { delay_ms: 375, feedback: 0.3 }
}
```

This is equivalent to writing `delay_ms[0]: 250, feedback[0]: 0.4, …`
explicitly. Alias-based indexing is also supported when the module shape
defines named aliases:

```patches
module mix : Mixer(channels: [drums, bass, lead]) {
    @drums: { level: 0.8 },
    @bass:  { level: 0.6 }
}
```

## Whitespace and comments

Whitespace is insignificant. Comments run from `#` to end of line.
