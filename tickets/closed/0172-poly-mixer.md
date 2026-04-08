---
id: "0172"
title: "PolyMixer: poly N-channel mixer with level, mute/solo"
priority: medium
epic: E029
created: 2026-03-20
---

## Summary

Implement `patches_modules::mixer::PolyMixer`, a polyphonic mixing module whose
channel count is set by `ModuleShape::channels`. Each channel takes a poly input and
has independent level, mute, and solo controls. The module produces a single poly
output that is the per-voice sum of all active channels.

No send/receive loop ports are included (see E029 for rationale).

## Port layout

### Inputs (N = `channels`)

| Name       | Indices | Kind | Description                  |
|------------|---------|------|------------------------------|
| `in`       | 0..N−1  | Poly | Per-channel poly audio input |
| `level_cv` | 0..N−1  | Mono | Additive CV for level        |

### Parameters (per channel i)

| Name    | Index | Type  | Range  | Default | Description         |
|---------|-------|-------|--------|---------|---------------------|
| `level` | i     | Float | [0, 1] | 1.0     | Channel fader level |
| `mute`  | i     | Bool  | —      | false   | Silence channel     |
| `solo`  | i     | Bool  | —      | false   | Solo channel        |

### Outputs

| Name  | Index | Kind | Description                      |
|-------|-------|------|----------------------------------|
| `out` | 0     | Poly | Per-voice sum of active channels |

## Signal flow

```
effective_level = clamp(level[i] + level_cv[i], 0, 1)
channel_active  = !mute[i] && (!any_solo || solo[i])

out[v] = Σ (in[i][v] × effective_level[i] × channel_active[i])   for each voice v
```

The `level_cv` is a mono value applied uniformly to all voices of that channel.

## Implementation notes

- `in` ports: `poly_in_multi("in", N)`
- `level_cv` ports: `mono_in_multi("level_cv", N)`
- Store `Vec<PolyInput>` for `in` and `Vec<MonoInput>` for `level_cv`.
- In `process`, accumulate into a `[f32; 16]` scratch buffer, then write via
  `pool.write_poly`.
- Cache `effective_level` and `channel_active` scalars in `process` to avoid
  repeated parameter lookups (they are set in `update_validated_parameters`).

## Acceptance criteria

- [ ] `PolyMixer` compiles with no `clippy` warnings.
- [ ] `describe` returns correct port counts (N poly inputs + N mono CV inputs, 1 poly output).
- [ ] Unit tests:
  - [ ] Descriptor shape for N=2.
  - [ ] Two channels at unity level, no mute/solo: each voice sums both inputs.
  - [ ] Level scales each channel independently (and per-voice result is correct).
  - [ ] Level CV clamps at 1.0.
  - [ ] Muting a channel zeroes its contribution per voice.
  - [ ] Soloing one channel silences the others.
  - [ ] Mute overrides solo.
- [ ] `cargo test -p patches-modules` passes.
- [ ] `cargo clippy -p patches-modules` passes.

## Notes

See E029 for design overview. Module registration happens in T-0174.
