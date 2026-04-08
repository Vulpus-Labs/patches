# Mixers

> **Source of truth:** the doc comments on each module struct in
> `patches-modules/src/` define the canonical port names, parameter
> ranges, and behaviour. This page is kept in sync with those comments.

---

## `Sum` — Unweighted mono sum

Sums N mono inputs into one mono output with no level control.

```patches
module mix : Sum(channels: 4)
```

**Inputs**

| Port | Description |
| --- | --- |
| `in[0]` … `in[n-1]` | Mono inputs |

**Outputs**

| Port | Description |
| --- | --- |
| `out` | Sum of all inputs |

---

## `Mixer` — Mono mixer with sends and mute/solo

N-channel mono mixer. Each channel has level CV, two auxiliary send buses,
and mute/solo. If any channel is soloed, only soloed (non-muted) channels
contribute to the output.

```patches
module mix : Mixer(channels: 4)
```

**Parameters** (all indexed per channel)

| Parameter | Type | Default | Description |
| --- | --- | --- | --- |
| `level[n]` | float | `1.0` | Channel gain (0–1) |
| `send_a[n]` | float | `0.0` | Send A amount (0–1) |
| `send_b[n]` | float | `0.0` | Send B amount (0–1) |
| `mute[n]` | bool | `false` | Mute this channel |
| `solo[n]` | bool | `false` | Solo this channel |

**Inputs**

| Port | Description |
| --- | --- |
| `in[0]` … `in[n-1]` | Mono audio inputs |
| `level_cv[0]` … `level_cv[n-1]` | Per-channel level CV (added to `level`) |
| `send_a_cv[0]` … `send_a_cv[n-1]` | Per-channel send A CV |
| `send_b_cv[0]` … `send_b_cv[n-1]` | Per-channel send B CV |
| `return_a` | Return input added to main output |
| `return_b` | Return input added to main output |

**Outputs**

| Port | Description |
| --- | --- |
| `out` | Main mono mix |
| `send_a` | Send A bus output |
| `send_b` | Send B bus output |

---

## `StereoMixer` — Stereo mixer with pan, sends and mute/solo

N-channel stereo mixer. Each channel has level, pan, two send buses, and
mute/solo.

```patches
module mix : StereoMixer(channels: 4)
```

**Parameters** (all indexed per channel)

| Parameter | Type | Default | Description |
| --- | --- | --- | --- |
| `level[n]` | float | `1.0` | Channel gain (0–1) |
| `pan[n]` | float | `0.0` | Pan (−1 = full left, +1 = full right) |
| `send_a[n]` | float | `0.0` | Send A amount (0–1) |
| `send_b[n]` | float | `0.0` | Send B amount (0–1) |
| `mute[n]` | bool | `false` | Mute this channel |
| `solo[n]` | bool | `false` | Solo this channel |

**Inputs**

| Port | Description |
| --- | --- |
| `in[0]` … `in[n-1]` | Mono audio inputs |
| `level_cv[0]` … `level_cv[n-1]` | Per-channel level CV |
| `pan_cv[0]` … `pan_cv[n-1]` | Per-channel pan CV |
| `send_a_cv[0]` … `send_a_cv[n-1]` | Per-channel send A CV |
| `send_b_cv[0]` … `send_b_cv[n-1]` | Per-channel send B CV |
| `return_a_left` / `return_a_right` | Send A stereo return |
| `return_b_left` / `return_b_right` | Send B stereo return |

**Outputs**

| Port | Description |
| --- | --- |
| `out_left` / `out_right` | Main stereo output |
| `send_a_left` / `send_a_right` | Send A stereo bus |
| `send_b_left` / `send_b_right` | Send B stereo bus |

---

## `PolyMix` — Unweighted poly sum

Sums N poly inputs voice-by-voice with no level control.

```patches
module mix : PolyMix(channels: 2)
```

**Inputs**

| Port | Description |
| --- | --- |
| `in[0]` … `in[n-1]` | Poly inputs |

**Outputs**

| Port | Description |
| --- | --- |
| `out` | Per-voice sum (poly) |

---

## `PolyMixer` — Poly mixer with level and mute/solo

N-channel poly mixer with per-channel level, mute, and solo. Level CV inputs
are mono (applied uniformly across all voices of that channel).

```patches
module mix : PolyMixer(channels: 2)
```

**Parameters** (indexed per channel)

| Parameter | Type | Default | Description |
| --- | --- | --- | --- |
| `level[n]` | float | `1.0` | Channel gain (0–1) |
| `mute[n]` | bool | `false` | Mute this channel |
| `solo[n]` | bool | `false` | Solo this channel |

**Inputs**

| Port | Description |
| --- | --- |
| `in[0]` … `in[n-1]` | Poly audio inputs |
| `level_cv[0]` … `level_cv[n-1]` | Per-channel level CV (mono) |

**Outputs**

| Port | Description |
| --- | --- |
| `out` | Per-voice sum (poly) |

---

## `StereoPolyMixer` — Stereo poly mixer with pan and mute/solo

N-channel stereo poly mixer. Outputs two poly cables (left and right).
Level and pan CV inputs are mono.

```patches
module mix : StereoPolyMixer(channels: 2)
```

**Parameters** (indexed per channel)

| Parameter | Type | Default | Description |
| --- | --- | --- | --- |
| `level[n]` | float | `1.0` | Channel gain (0–1) |
| `pan[n]` | float | `0.0` | Pan (−1 = full left, +1 = full right) |
| `mute[n]` | bool | `false` | Mute this channel |
| `solo[n]` | bool | `false` | Solo this channel |

**Inputs**

| Port | Description |
| --- | --- |
| `in[0]` … `in[n-1]` | Poly audio inputs |
| `level_cv[0]` … `level_cv[n-1]` | Per-channel level CV (mono) |
| `pan_cv[0]` … `pan_cv[n-1]` | Per-channel pan CV (mono) |

**Outputs**

| Port | Description |
| --- | --- |
| `out_left` | Per-voice left output (poly) |
| `out_right` | Per-voice right output (poly) |

---

## `PolyToMono` — Collapse poly to mono

Sums all active voices of a poly cable into a single mono signal.

**Inputs**

| Port | Description |
| --- | --- |
| `in` | Poly input |

**Outputs**

| Port | Description |
| --- | --- |
| `out` | Mono sum of all voices |

---

## `MonoToPoly` — Broadcast mono to all voices

Copies a mono signal into every voice slot of a poly cable.

**Inputs**

| Port | Description |
| --- | --- |
| `in` | Mono input |

**Outputs**

| Port | Description |
| --- | --- |
| `out` | Poly cable (same value in every voice) |
