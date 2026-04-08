# Connections & scaling

## Basic connection

```patches
osc.sine -> out.in_left
```

Reads the `sine` output of `osc` and writes it to the `in_left` input of `out`.

## Backward connections

Inside templates, `<-` connects from right to left — useful for wiring
boundary ports:

```patches
$.audio <- vca.out
```

This is equivalent to `vca.out -> $.audio`.

## Scaled connection

```patches
mix.out -[0.1]-> out.in_left
```

The value in brackets is a scale factor multiplied onto the signal before it
reaches the destination input. This is applied at the cable level with no extra
processing cost.

Useful values:

| Scale | Effect |
|---|---|
| `1.0` | unity (same as unscaled `->`) |
| `0.5` | attenuate by half |
| `-1.0` | invert (phase flip) |
| `0.1` | strong attenuation (e.g. audio → FM input) |

Backward arrows support scales too: `<-[0.5]-`.

Inside templates, the scale can be a parameter reference:

```patches
template attenuated(gain: float = 0.5) {
    in: input
    out: output

    $.output <-[<gain>]- $.input
}
```

## Multiple connections to one input

Each input port accepts exactly one incoming cable. Connecting a second cable to
an already-connected input is an error at patch-build time:

```text
input port "in/0" on node "vca" already has a connection
```

To combine signals before an input, use a mixer module explicitly:

```patches
module mix : MonoMixer
module vca : Vca

osc_a.sine -> mix.in[0]
osc_b.sine -> mix.in[1]
mix.out -> vca.input
```

## Fan-out

One output can drive multiple inputs:

```patches
osc.sine -> filter.in
osc.sine -> out.in_left
```

The output value is read once per tick; all destinations see the same value.

## Cable delay

Cables have a **1-sample delay**. This means the value written by a module in
tick N is available to all downstream modules in tick N+1. This is intentional:
it allows modules to run in any order within a tick without requiring topological
sorting, and it makes feedback connections well-defined.
