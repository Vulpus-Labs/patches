# Primitives

> **Source of truth:** the doc comments on each module struct in
> `patches-modules/src/` define the canonical port names, parameter
> ranges, and behaviour. This page is kept in sync with those comments.

Small DSP building blocks that are useful on their own and as
components inside larger patches. See [ADR
0076](../../../adr/0076-native-dynamics-and-stereo-utility-modules.md).

## `DcBlocker` — One-pole DC removal

Thin wrapper over the shared `patches_dsp::DcBlocker` kernel: a
one-pole highpass at ~5 Hz. No parameters; the cutoff is fixed by the
kernel. Useful after waveshapers, bias injection, or any process that
may introduce a DC offset.

**Inputs**

| Port | Kind | Description |
| ---- | ---- | ----------- |
| `in` | mono | Audio input |

**Outputs**

| Port  | Kind | Description       |
| ----- | ---- | ----------------- |
| `out` | mono | DC-blocked output |

## `Comb` — Universal comb (FF / FB / both)

Single module covering feed-forward (FIR), feedback (IIR), and combined
pole-zero comb topologies via a `mode` enum. The `feedback` parameter
is the coefficient on the delayed tap; its meaning depends on `mode`:

```text
ff   : y[n] = mix · (x[n] + g · x[n−D])
fb   : y[n] = mix · y'[n]              where y'[n] = x[n] + g · y'[n−D]
both : y[n] = mix · y'[n]              where y'[n] = x[n] + g · x[n−D] + g · y'[n−D]
```

`y'` is the pre-mix signal — feedback recursion uses the unmixed value
so adjusting `mix` does not destabilise the loop.

In modes that include `fb` the caller is responsible for keeping
`feedback` strictly below `1.0`. The module does not clamp on the user's
behalf; `feedback ≥ 1` produces unbounded growth (the same convention
as other recursive modules in the bundle).

**Parameters**

| Parameter  | Type            | Default | Description                                        |
| ---------- | --------------- | ------- | -------------------------------------------------- |
| `mode`     | `ff`/`fb`/`both` | `fb`    | Topology selection                                 |
| `delay_ms` | float (ms)      | `10.0`  | Delay-line length (0.1–500 ms)                     |
| `feedback` | float           | `0.5`   | Delayed-tap coefficient (−0.99..0.99)              |
| `mix`      | float           | `1.0`   | Output scale applied after the comb topology       |

**Inputs**

| Port | Kind | Description |
| ---- | ---- | ----------- |
| `in` | mono | Audio input |

**Outputs**

| Port  | Kind | Description |
| ----- | ---- | ----------- |
| `out` | mono | Comb output |
