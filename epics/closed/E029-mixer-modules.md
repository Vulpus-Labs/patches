---
id: "E029"
title: Mixer modules (Mixer, StereoMixer, PolyMixer, StereoPolyMixer)
status: closed
priority: medium
created: 2026-03-20
tickets:
  - "0170"
  - "0171"
  - "0172"
  - "0173"
  - "0174"
---

## Summary

Adds four mixing modules to `patches-modules`, each with a configurable number of
channels driven by `ModuleShape::channels`:

| Module            | Input kind | Pan | Send/receive loops |
|-------------------|------------|-----|--------------------|
| `Mixer`           | Mono       | No  | Yes                |
| `StereoMixer`     | Mono       | Yes | Yes                |
| `PolyMixer`       | Poly       | No  | No                 |
| `StereoPolyMixer` | Poly       | Yes | No                 |

Send/receive loops are omitted from the poly variants because there is no
practical use case for poly pre-fader sends.

## Per-channel controls

Every channel has:

- **`in/i`** — audio input (Mono or Poly depending on variant)
- **`level/i`** — level parameter [0, 1], default 1.0
- **`level_cv/i`** — additive CV for level (MonoInput); result clamped to [0, 1]
- **`mute/i`** — boolean parameter (default false)
- **`solo/i`** — boolean parameter (default false)

Stereo variants additionally have per-channel:

- **`pan/i`** — pan parameter [−1, 1] where −1 = full left, +1 = full right, default 0.0
- **`pan_cv/i`** — additive CV for pan (MonoInput); result clamped to [−1, 1]

Non-poly variants additionally have per-channel:

- **`send_a/i`**, **`send_b/i`** — send level parameters [0, 1], default 0.0
- **`send_a_cv/i`**, **`send_b_cv/i`** — additive CV for sends (MonoInput); clamped to [0, 1]

## Module-level ports

Mono (`Mixer`):
- **`receive_a/0`**, **`receive_b/0`** — MonoInput; added directly to main output
- **`out/0`** — MonoOutput
- **`send_a/0`**, **`send_b/0`** — MonoOutput send buses

Stereo (`StereoMixer`):

- **`receive_a_left/0`**, **`receive_a_right/0`**, **`receive_b_left/0`**, **`receive_b_right/0`** — MonoInput; added to the corresponding L/R main bus
- **`out_left/0`**, **`out_right/0`** — MonoOutput
- **`send_a_left/0`**, **`send_a_right/0`**, **`send_b_left/0`**, **`send_b_right/0`** — MonoOutput stereo send buses (post-pan, post-level)

Poly variants:
- **`out/0`** (PolyMixer) or **`out_left/0`** + **`out_right/0`** (StereoPolyMixer) — PolyOutput

## Mute/solo semantics

- If any channel has `solo = true`, only soloed channels contribute to the output.
- `mute` silences a channel regardless of solo state.
- A muted-and-soloed channel is silent (mute wins).

## Pan law

Linear equal-gain: `left_gain = (1 − pan) × 0.5`, `right_gain = (1 + pan) × 0.5`.
At centre (pan = 0) both gains are 0.5, so a centred mono source is −6 dBFS per side.
This is the simplest law and consistent with a summing mixer model; a constant-power
law can be substituted later without changing the interface.

## Signal flow (non-poly)

```
effective_level  = clamp(level_param  + level_cv,  0, 1)
effective_send_a = clamp(send_a_param + send_a_cv, 0, 1)
effective_send_b = clamp(send_b_param + send_b_cv, 0, 1)
channel_active   = !mute && (!any_solo || solo)

out       = Σ (in_i × effective_level_i × channel_active_i) + receive_a + receive_b
send_a    = Σ (in_i × effective_send_a_i × channel_active_i)
send_b    = Σ (in_i × effective_send_b_i × channel_active_i)
```

For stereo, `in_i × effective_level_i` is scaled by `left_gain_i` / `right_gain_i`
before accumulation into separate left/right buses.

## Tickets

- [T-0170](../tickets/open/0170-mixer.md) — `Mixer`: mono N-channel with level, sends, mute/solo
- [T-0171](../tickets/open/0171-stereo-mixer.md) — `StereoMixer`: stereo N-channel with pan, sends, mute/solo
- [T-0172](../tickets/open/0172-poly-mixer.md) — `PolyMixer`: poly N-channel with level, mute/solo
- [T-0173](../tickets/open/0173-stereo-poly-mixer.md) — `StereoPolyMixer`: stereo poly N-channel with pan, mute/solo
- [T-0174](../tickets/open/0174-mixer-registry-and-tests.md) — Register all mixer types; integration tests
