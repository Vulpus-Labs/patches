# Stereo utilities

> **Source of truth:** the doc comments on each module struct in
> `patches-modules/src/stereo/` define the canonical port names,
> parameter ranges, and behaviour. This page is kept in sync with those
> comments.

The stereo utility group covers image control — pan, balance, width,
mid-side encode/decode, and a sum-mono-bass crossover. `StereoSplitter`,
`StereoJoiner`, and `StereoSum` complement these but live elsewhere
today; they will migrate into this group under the source-tree
reorganisation tickets.

## `Pan` — Equal-power mono pan

Places a mono signal in the stereo field using the equal-power law
`(L, R) = in · (cos θ, sin θ)` with `θ = (pan + 1) · π/4`. The total
power `L² + R²` is preserved across the sweep; at the centre each
channel receives `in / √2` (−3 dB). The CV input is added to the
parameter and clamped to `[-1, 1]` before the angle is computed.

**Parameters**

| Parameter | Type  | Default | Description                          |
| --------- | ----- | ------- | ------------------------------------ |
| `pan`     | float | `0.0`   | Base position; -1 = L, +1 = R (-1–1) |

**Inputs**

| Port  | Description                       |
| ----- | --------------------------------- |
| `in`  | Mono source                       |
| `pan` | Additive CV (offsets `pan` param) |

**Outputs**

| Port  | Description            |
| ----- | ---------------------- |
| `out` | Equal-power stereo L/R |

---

## `Balance` — Linear stereo balance

Attenuates one side of a stereo signal while the other passes unchanged.
With `balance ∈ [-1, 1]`, `gain_L = clamp(1 − balance, 0, 1)` and
`gain_R = clamp(1 + balance, 0, 1)`. At the centre the signal passes
through unchanged; at the extremes the opposite side is silenced. The
linear ramp per side matches the mixer's pan-law ramp shape. The CV
input is added to the parameter before clamping.

**Parameters**

| Parameter | Type  | Default | Description                          |
| --------- | ----- | ------- | ------------------------------------ |
| `balance` | float | `0.0`   | Base balance; -1 = L, +1 = R (-1–1)  |

**Inputs**

| Port      | Description                              |
| --------- | ---------------------------------------- |
| `in`      | Stereo source                            |
| `balance` | Additive CV (offsets `balance` param)    |

**Outputs**

| Port  | Description       |
| ----- | ----------------- |
| `out` | Balanced stereo   |

---

## `StereoWidth` — Width control via internal M-S

Encodes the input to mid (`M = (L+R)/2`) and side (`S = (L-R)/2`),
scales `S` by `width`, leaves `M` unchanged, and decodes back:
`L_out = M + width·S`, `R_out = M − width·S`. At `width = 0` both
outputs collapse to the mono sum; at `width = 1` the implementation
short-circuits so the M-S round-trip is bit-exact; at `width = 2`
antiphase content is doubled. The CV input is added to the parameter
and clamped to `[0, 2]` before scaling.

**Parameters**

| Parameter | Type  | Default | Description                                              |
| --------- | ----- | ------- | -------------------------------------------------------- |
| `width`   | float | `1.0`   | 0 = mono sum, 1 = unchanged, 2 = double-wide (0–2)       |

**Inputs**

| Port    | Description                            |
| ------- | -------------------------------------- |
| `in`    | Stereo source                          |
| `width` | Additive CV (offsets `width` param)    |

**Outputs**

| Port  | Description           |
| ----- | --------------------- |
| `out` | Width-scaled stereo   |

---

## `MidSide` — Bidirectional encode/decode

Two independent arithmetic paths in one module: the encode side reads
`stereo_in` (L/R) and writes `(M, S) = ((L+R)/2, (L-R)/2)` to `ms_out`;
the decode side reads `ms_in` (M/S) and writes `(M+S, M−S)` to
`stereo_out`. Either side can be used in isolation by leaving the other
side's ports unconnected.

All four ports are stereo cables. The mid-side form is just a stereo
cable carrying `(M, S)` rather than `(L, R)` — nothing in the
descriptor distinguishes the two, the same constraint that already
applies to any mid/side workflow in a DAW.

**Inputs**

| Port        | Description                  |
| ----------- | ---------------------------- |
| `stereo_in` | L/R input to encode side     |
| `ms_in`     | (M, S) input to decode side  |

**Outputs**

| Port         | Description              |
| ------------ | ------------------------ |
| `ms_out`     | (M, S) from encode side  |
| `stereo_out` | L/R from decode side     |

---

## `MonoBass` — Mono-summed bass via LR4 crossover

Splits the stereo input into a low band and a high band at `cutoff`
using Linkwitz-Riley 4th-order filters (two cascaded Butterworth
biquads each). The lows are summed to mono (`(L+R)/2` then LP-filtered)
and sent identically to both output channels; the highs are HP-filtered
per channel so the stereo image above the crossover is preserved. At
the crossover frequency, the LP and HP magnitudes are each −6 dB; the
sum-flat property means an L = R input is reconstructed at unity
magnitude while differential content below the cutoff is summed away.

Use for vinyl-safe masters, club-system protection, and any source
whose low end you don't trust to be summable.

**Parameters**

| Parameter | Type     | Default | Description                          |
| --------- | -------- | ------- | ------------------------------------ |
| `cutoff`  | float Hz | `120.0` | Crossover frequency (20–2000 Hz)     |

**Inputs**

| Port | Description   |
| ---- | ------------- |
| `in` | Stereo source |

**Outputs**

| Port  | Description                              |
| ----- | ---------------------------------------- |
| `out` | High-band stereo + low-band mono on both |
