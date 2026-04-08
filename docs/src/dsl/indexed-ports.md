# Indexed ports

Some modules expose multiple ports of the same kind, addressed by index.
The notation uses square brackets:

```patches
mix.in[0]
mix.in[1]
mix.in[2]
```

## Connecting to indexed ports

```patches
osc_a.sine -> mix.in[0]
osc_b.sine -> mix.in[1]
osc_c.sine -> mix.in[2]
```

Scaled connections work the same way:

```patches
osc_a.sine -[0.5]-> mix.in[0]
```

## Index range

The valid index range is determined by the module's arity, declared when the
module is instantiated:

```patches
module mix : StereoMixer(channels: 3)
# valid inputs: mix.in[0], mix.in[1], mix.in[2]
```

Connecting to an out-of-range index is a validation error caught at parse time.
