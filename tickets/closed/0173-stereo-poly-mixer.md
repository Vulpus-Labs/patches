---
id: "0173"
title: "StereoPolyMixer: stereo poly N-channel mixer with pan, mute/solo"
priority: medium
epic: E029
created: 2026-03-20
---

## Summary

Implement `patches_modules::mixer::StereoPolyMixer`, extending `PolyMixer` with
per-channel panning. Each poly channel is spread across left and right poly output
buses using the same linear pan law as `StereoMixer`. No send/receive loop ports.

## Port layout

### Inputs (N = `channels`)

| Name       | Indices | Kind | Description                  |
|------------|---------|------|------------------------------|
| `in`       | 0..N−1  | Poly | Per-channel poly audio input |
| `level_cv` | 0..N−1  | Mono | Additive CV for level        |
| `pan_cv`   | 0..N−1  | Mono | Additive CV for pan          |

### Parameters (per channel i)

| Name    | Index | Type  | Range   | Default | Description         |
|---------|-------|-------|---------|---------|---------------------|
| `level` | i     | Float | [0, 1]  | 1.0     | Channel fader level |
| `pan`   | i     | Float | [−1, 1] | 0.0     | Pan position        |
| `mute`  | i     | Bool  | —       | false   | Silence channel     |
| `solo`  | i     | Bool  | —       | false   | Solo channel        |

### Outputs

| Name        | Index | Kind | Description             |
|-------------|-------|------|-------------------------|
| `out_left`  | 0     | Poly | Per-voice left bus sum  |
| `out_right` | 0     | Poly | Per-voice right bus sum |

## Signal flow

```
effective_level = clamp(level[i] + level_cv[i], 0, 1)
effective_pan   = clamp(pan[i]   + pan_cv[i],   −1, 1)
left_gain[i]    = (1 − effective_pan) × 0.5
right_gain[i]   = (1 + effective_pan) × 0.5
channel_active  = !mute[i] && (!any_solo || solo[i])
scale[i]        = effective_level[i] × channel_active[i]

out_left[v]  = Σ (in[i][v] × scale[i] × left_gain[i])
out_right[v] = Σ (in[i][v] × scale[i] × right_gain[i])
```

The mono `level_cv` and `pan_cv` are applied uniformly to all voices of the channel.

## Acceptance criteria

- [ ] `StereoPolyMixer` compiles with no `clippy` warnings.
- [ ] `describe` returns correct port counts (N poly inputs + 2×N mono CV inputs, 2 poly outputs).
- [ ] Unit tests:
  - [ ] Descriptor shape for N=2.
  - [ ] Centre pan: signal split equally left/right for each voice.
  - [ ] Full-left pan: all signal to out_left, out_right = 0 per voice.
  - [ ] Full-right pan: all signal to out_right, out_left = 0 per voice.
  - [ ] Pan CV clamps at ±1.
  - [ ] Level scales both buses together.
  - [ ] Mute/solo behaviour correct.
- [ ] `cargo test -p patches-modules` passes.
- [ ] `cargo clippy -p patches-modules` passes.

## Notes

See E029. Module registration happens in T-0174.
