# Delays & Reverb

> **Source of truth:** the doc comments on each module struct in
> `patches-modules/src/` define the canonical port names, parameter
> ranges, and behaviour. This page is kept in sync with those comments.

---

## `Delay` — Mono multi-tap delay

One shared 4-second delay buffer with N independent read taps. Each tap has its
own delay time, gain, feedback, tone, and drive. All tap feedbacks sum back into
the buffer write before the next sample. N is set by `channels`.

```patches
module dly : Delay(channels: 2) {
    delay_ms[0]: 250,
    delay_ms[1]: 375,
    feedback[0]: 0.4,
    feedback[1]: 0.3,
    dry_wet:     1.0
}
```

**Parameters** (global)

| Parameter | Type | Range | Default | Description |
| --- | --- | --- | --- | --- |
| `dry_wet` | float | 0–1 | `1.0` | Crossfade between dry input and wet tap sum |

**Parameters** (per tap `i`)

| Parameter | Type | Range | Default | Description |
| --- | --- | --- | --- | --- |
| `delay_ms[i]` | int | 0–2000 | `500` | Tap delay time in milliseconds |
| `gain[i]` | float | 0–1 | `1.0` | Tap output gain |
| `feedback[i]` | float | 0–1 | `0.0` | Tap feedback amount (sent back into write) |
| `tone[i]` | float | 0–1 | `1.0` | High-frequency roll-off in the feedback path (1 = bright, 0 = dark) |
| `drive[i]` | float | 0.1–10 | `1.0` | Soft-clip drive in the feedback path |

**Inputs**

| Port | Description |
| --- | --- |
| `in` | Mono audio input |
| `drywet_cv` | Additive CV for `dry_wet` |
| `delay_cv[0]` … `delay_cv[n-1]` | Additive CV for delay time (±1 scales ±100%) |
| `gain_cv[0]` … `gain_cv[n-1]` | Additive CV for tap gain |
| `fb_cv[0]` … `fb_cv[n-1]` | Additive CV for feedback amount |
| `return[0]` … `return[n-1]` | Pre-gain return signal per tap (added after the raw tap read) |

**Outputs**

| Port | Description |
| --- | --- |
| `out` | Dry/wet mixed output |
| `send[0]` … `send[n-1]` | Pre-gain tap signal per tap (before `return` mixing) |

---

## `StereoDelay` — Stereo multi-tap delay

Two 4-second delay buffers (L and R) sharing a single set of N read taps. Each
tap has pan, and an optional pingpong mode that cross-routes feedback (L→R,
R→L). N is set by `channels`.

```patches
module dly : StereoDelay(channels: 2) {
    delay_ms[0]: 300,
    delay_ms[1]: 450,
    feedback[0]: 0.35,
    feedback[1]: 0.35,
    pingpong[0]: true,
    pingpong[1]: true,
    pan[0]:      -0.3,
    pan[1]:       0.3
}
```

**Parameters** (global)

| Parameter | Type | Range | Default | Description |
| --- | --- | --- | --- | --- |
| `dry_wet` | float | 0–1 | `1.0` | Crossfade between dry input and wet tap sum |

**Parameters** (per tap `i`)

| Parameter | Type | Range | Default | Description |
| --- | --- | --- | --- | --- |
| `delay_ms[i]` | int | 0–2000 | `500` | Tap delay time in milliseconds |
| `gain[i]` | float | 0–1 | `1.0` | Tap output gain |
| `feedback[i]` | float | 0–1 | `0.0` | Tap feedback amount |
| `tone[i]` | float | 0–1 | `1.0` | HF roll-off in feedback path (1 = bright, 0 = dark) |
| `drive[i]` | float | 0.1–10 | `1.0` | Soft-clip drive in feedback path |
| `pan[i]` | float | −1–+1 | `0.0` | Stereo pan position (−1 = full left, +1 = full right) |
| `pingpong[i]` | bool | — | `false` | Cross-route feedback: L feedback→R buffer, R feedback→L buffer |

**Inputs**

| Port | Description |
| --- | --- |
| `in_left` | Left audio input |
| `in_right` | Right audio input |
| `drywet_cv` | Additive CV for `dry_wet` |
| `delay_cv[0]` … `delay_cv[n-1]` | Additive CV for delay time |
| `gain_cv[0]` … `gain_cv[n-1]` | Additive CV for tap gain |
| `fb_cv[0]` … `fb_cv[n-1]` | Additive CV for feedback amount |
| `pan_cv[0]` … `pan_cv[n-1]` | Additive CV for tap pan |
| `return_left[0]` … `return_left[n-1]` | Pre-gain L return per tap |
| `return_right[0]` … `return_right[n-1]` | Pre-gain R return per tap |

**Outputs**

| Port | Description |
| --- | --- |
| `out_left` | Left dry/wet mixed output |
| `out_right` | Right dry/wet mixed output |
| `send_left[0]` … `send_left[n-1]` | Pre-gain L tap signal per tap |
| `send_right[0]` … `send_right[n-1]` | Pre-gain R tap signal per tap |

---

## `FdnReverb` — Stereo FDN reverb

An 8-line feedback delay network (FDN) with Hadamard mixing matrix, per-line
high-shelf absorption, Thiran all-pass interpolation for LFO-modulated reads,
and stereo output via orthogonal output gain vectors. Five character archetypes
control the room model; `size` and `brightness` sweep within each archetype.

```patches
module verb : FdnReverb {
    character: hall,
    size:       0.6,
    brightness: 0.5,
    pre_delay:  0.3,
    mix:        0.8
}
```

**Parameters**

| Parameter | Type | Range | Default | Description |
| --- | --- | --- | --- | --- |
| `character` | enum | `plate` / `room` / `chamber` / `hall` / `cathedral` | `hall` | Room archetype — sets delay-line scaling, LFO rate/depth, pre-delay length, and decay curve shape |
| `size` | float | 0–1 | `0.5` | Room size within the archetype: 0 = smallest/shortest, 1 = largest/longest |
| `brightness` | float | 0–1 | `0.5` | High-frequency decay ratio: 0 = dark (HF decays much faster), 1 = bright (HF/LF decay close together) |
| `pre_delay` | float | 0–1 | `0.0` | Additional pre-delay (additive with `size`): 0 = no extra, 1 = maximum for the archetype |
| `mix` | float | 0–1 | `1.0` | Dry/wet mix: 0 = fully dry, 1 = fully wet |

**Character archetypes**

| Name | RT60 range (LF) | Pre-delay | Character |
| --- | --- | --- | --- |
| `plate` | 0.3 s – 1.5 s | up to 10 ms | Dense, smooth; no sense of room geometry |
| `room` | 0.4 s – 2.5 s | up to 25 ms | Small to mid-size room with clear early reflections |
| `chamber` | 0.3 s – 2.0 s | up to 20 ms | Tight, controlled decay; studio chamber character |
| `hall` | 0.8 s – 5.0 s | up to 50 ms | Concert hall — the default |
| `cathedral` | 1.5 s – 8.0 s | up to 80 ms | Very long, diffuse reverb tail |

**Inputs**

| Port | Description |
| --- | --- |
| `in_left` | Left audio input (if unconnected, `in_right` is used for both channels) |
| `in_right` | Right audio input |
| `size_cv` | Additive CV for `size` |
| `brightness_cv` | Additive CV for `brightness` |
| `pre_delay_cv` | Additive CV for `pre_delay` |
| `mix_cv` | Additive CV for `mix` |

**Outputs**

| Port | Description |
| --- | --- |
| `out_left` | Left reverb output |
| `out_right` | Right reverb output |

---

## `ConvReverb` — Mono convolution reverb

Partitioned-convolution reverb driven by an impulse response (built-in
preset or loaded from a WAV file). See
`patches-modules/src/convolution_reverb/` for the current parameter set,
IR-loading mechanism, and port list.

## `StereoConvReverb` — Stereo convolution reverb

Stereo variant of `ConvReverb` with per-channel IR processing. See
`patches-modules/src/convolution_reverb/` for details.
