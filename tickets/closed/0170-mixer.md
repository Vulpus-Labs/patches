---
id: "0170"
title: "Mixer: mono N-channel mixer with level, sends, mute/solo"
priority: medium
epic: E029
created: 2026-03-20
---

## Summary

Implement `patches_modules::mixer::Mixer`, a mono mixing module whose channel count
is set by `ModuleShape::channels`. Each channel has independent level, send A/B,
mute, and solo controls. The module also has send A/B output buses and receive A/B
inputs for effects loops.

## Port layout

### Inputs (N = `channels`)

| Name         | Indices | Kind | Description                    |
|--------------|---------|------|--------------------------------|
| `in`         | 0..N−1  | Mono | Per-channel audio input        |
| `level_cv`   | 0..N−1  | Mono | Additive CV for level          |
| `send_a_cv`  | 0..N−1  | Mono | Additive CV for send A amount  |
| `send_b_cv`  | 0..N−1  | Mono | Additive CV for send B amount  |
| `receive_a`  | 0       | Mono | Return from send A effects     |
| `receive_b`  | 0       | Mono | Return from send B effects     |

All per-channel input groups are declared with `mono_in_multi(name, N)`.
`receive_a` and `receive_b` are declared after the per-channel groups with
`mono_in("receive_a")` and `mono_in("receive_b")`.

### Parameters (per channel i)

| Name       | Index | Type  | Range  | Default | Description           |
|------------|-------|-------|--------|---------|-----------------------|
| `level`    | i     | Float | [0, 1] | 1.0     | Channel fader level   |
| `send_a`   | i     | Float | [0, 1] | 0.0     | Send A amount         |
| `send_b`   | i     | Float | [0, 1] | 0.0     | Send B amount         |
| `mute`     | i     | Bool  | —      | false   | Silence this channel  |
| `solo`     | i     | Bool  | —      | false   | Solo this channel     |

### Outputs

| Name     | Index | Kind | Description                        |
|----------|-------|------|------------------------------------|
| `out`    | 0     | Mono | Summed main output + receive buses |
| `send_a` | 0     | Mono | Send A bus                         |
| `send_b` | 0     | Mono | Send B bus                         |

## Signal flow

```
effective_level  = clamp(level[i]  + level_cv[i],  0, 1)
effective_send_a = clamp(send_a[i] + send_a_cv[i], 0, 1)
effective_send_b = clamp(send_b[i] + send_b_cv[i], 0, 1)
any_solo         = any channel where solo == true && !mute
channel_active   = !mute[i] && (!any_solo || solo[i])

out    = Σ (in[i] × effective_level[i]  × channel_active[i]) + receive_a + receive_b
send_a = Σ (in[i] × effective_send_a[i] × channel_active[i])
send_b = Σ (in[i] × effective_send_b[i] × channel_active[i])
```

A muted-and-soloed channel does not contribute to the solo bus (mute wins).

## Implementation notes

- Store per-channel `MonoInput` vecs for the four input groups, plus two fixed
  `MonoInput` fields for `receive_a`/`receive_b`.
- Accumulate `mute` and `solo` booleans into `Vec<bool>` fields in
  `update_validated_parameters` so `process` avoids repeated parameter lookups.
- The `send_a`/`send_b` output names conflict with the per-channel parameter names
  `send_a`/`send_b` — the former are output ports, the latter are parameters; they
  are in separate namespaces so there is no collision.

## Acceptance criteria

- [ ] `Mixer` compiles with no `clippy` warnings.
- [ ] `describe` returns the correct port counts for an arbitrary `channels` value.
- [ ] Unit tests (in-module `#[cfg(test)]`):
  - [ ] Descriptor shape for N=2: correct input/output counts and port names.
  - [ ] All-unity levels, no mute/solo: output equals sum of inputs.
  - [ ] Level CV clamps correctly (CV pushes above 1.0 → output clamped).
  - [ ] Muting a channel silences it.
  - [ ] Soloing one channel silences the others.
  - [ ] Mute overrides solo (muted+soloed channel is silent).
  - [ ] Send A/B buses accumulate per-channel send amounts.
  - [ ] receive_a/receive_b are added to main output.
- [ ] `cargo test -p patches-modules` passes.
- [ ] `cargo clippy -p patches-modules` passes.

## Notes

See E029 for the full design and signal flow equations.
Module registration happens in T-0174.
