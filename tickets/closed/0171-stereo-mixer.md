---
id: "0171"
title: "StereoMixer: stereo N-channel mixer with pan, sends, mute/solo"
priority: medium
epic: E029
created: 2026-03-20
---

## Summary

Implement `patches_modules::mixer::StereoMixer`, extending the mono `Mixer` design
with per-channel panning. Each channel's mono input is spread across left and right
output buses using a linear pan law. Send/receive loop ports are retained.

## Port layout

### Inputs (N = `channels`)

| Name               | Indices | Kind | Description                      |
|--------------------|---------|------|----------------------------------|
| `in`               | 0..N−1  | Mono | Per-channel audio input          |
| `level_cv`         | 0..N−1  | Mono | Additive CV for level            |
| `pan_cv`           | 0..N−1  | Mono | Additive CV for pan              |
| `send_a_cv`        | 0..N−1  | Mono | Additive CV for send A amount    |
| `send_b_cv`        | 0..N−1  | Mono | Additive CV for send B amount    |
| `receive_a_left`   | 0       | Mono | Left return from send A effects  |
| `receive_a_right`  | 0       | Mono | Right return from send A effects |
| `receive_b_left`   | 0       | Mono | Left return from send B effects  |
| `receive_b_right`  | 0       | Mono | Right return from send B effects |

### Parameters (per channel i)

| Name     | Index | Type  | Range   | Default | Description         |
|----------|-------|-------|---------|---------|---------------------|
| `level`  | i     | Float | [0, 1]  | 1.0     | Channel fader level |
| `pan`    | i     | Float | [−1, 1] | 0.0     | Pan position        |
| `send_a` | i     | Float | [0, 1]  | 0.0     | Send A amount       |
| `send_b` | i     | Float | [0, 1]  | 0.0     | Send B amount       |
| `mute`   | i     | Bool  | —       | false   | Silence channel     |
| `solo`   | i     | Bool  | —       | false   | Solo channel        |

### Outputs

| Name             | Index | Kind | Description                                  |
|------------------|-------|------|----------------------------------------------|
| `out_left`       | 0     | Mono | Left main output + left receive returns      |
| `out_right`      | 0     | Mono | Right main output + right receive returns    |
| `send_a_left`    | 0     | Mono | Send A left bus (post-pan, post-level)       |
| `send_a_right`   | 0     | Mono | Send A right bus (post-pan, post-level)      |
| `send_b_left`    | 0     | Mono | Send B left bus (post-pan, post-level)       |
| `send_b_right`   | 0     | Mono | Send B right bus (post-pan, post-level)      |

## Signal flow

```
effective_level  = clamp(level[i]  + level_cv[i],  0, 1)
effective_pan    = clamp(pan[i]    + pan_cv[i],    −1, 1)
effective_send_a = clamp(send_a[i] + send_a_cv[i], 0, 1)
effective_send_b = clamp(send_b[i] + send_b_cv[i], 0, 1)

left_gain[i]  = (1 − effective_pan) × 0.5
right_gain[i] = (1 + effective_pan) × 0.5

channel_active = !mute[i] && (!any_solo || solo[i])
scaled_left[i] = in[i] × effective_level[i] × left_gain[i]  × channel_active[i]
scaled_rght[i] = in[i] × effective_level[i] × right_gain[i] × channel_active[i]

out_left      = Σ scaled_left[i]  + receive_a_left  + receive_b_left
out_right     = Σ scaled_rght[i]  + receive_a_right + receive_b_right
send_a_left   = Σ (scaled_left[i]  × effective_send_a[i])
send_a_right  = Σ (scaled_rght[i]  × effective_send_a[i])
send_b_left   = Σ (scaled_left[i]  × effective_send_b[i])
send_b_right  = Σ (scaled_rght[i]  × effective_send_b[i])
```

Send buses are post-pan and post-level, so effects processors receive a
stereo-panned signal. Receives are added directly to the corresponding
left/right main bus.

## Acceptance criteria

- [ ] `StereoMixer` compiles with no `clippy` warnings.
- [ ] `describe` returns correct port counts (5×N inputs + 4 fixed inputs, 6 outputs).
- [ ] Unit tests:
  - [ ] Descriptor shape for N=2.
  - [ ] Centre pan (pan=0): signal split equally left/right in out and sends.
  - [ ] Full-left pan (pan=−1): all signal to left buses, right buses = 0.
  - [ ] Full-right pan (pan=+1): all signal to right buses, left buses = 0.
  - [ ] Pan CV clamps at ±1.
  - [ ] Mute/solo behaviour mirrors `Mixer`.
  - [ ] Send buses reflect pan position (post-pan).
  - [ ] receive_a_{left,right} and receive_b_{left,right} each added to the correct main output bus.
- [ ] `cargo test -p patches-modules` passes.
- [ ] `cargo clippy -p patches-modules` passes.

## Notes

See E029 for pan law rationale. Module registration happens in T-0174.
