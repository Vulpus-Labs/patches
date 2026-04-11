---
id: "0328"
title: Kick drum module
priority: high
created: 2026-04-12
---

## Summary

Implement a self-contained kick drum module producing a pitched, 808-style
kick from a trigger input. All synthesis (oscillator, pitch envelope,
amplitude envelope, saturation) is internal.

## Design

The kick sound is a sine oscillator with a fast pitch sweep from a high
frequency down to a settable base pitch, shaped by an amplitude decay
envelope, with optional saturation for grit.

### Inputs

| Port      | Kind | Description          |
| --------- | ---- | -------------------- |
| `trigger` | mono | Rising edge triggers |

### Outputs

| Port  | Kind | Description  |
| ----- | ---- | ------------ |
| `out` | mono | Kick signal  |

### Parameters

| Name         | Type  | Range         | Default | Description                          |
| ------------ | ----- | ------------- | ------- | ------------------------------------ |
| `pitch`      | float | 20–200 Hz     | 55      | Base pitch of the kick               |
| `sweep`      | float | 0–5000 Hz     | 2500    | Starting frequency of pitch sweep    |
| `sweep_time` | float | 0.001–0.5 s   | 0.04    | Duration of pitch sweep              |
| `decay`      | float | 0.01–2.0 s    | 0.5     | Amplitude decay time                 |
| `drive`      | float | 0.0–1.0       | 0.0     | Saturation amount                    |
| `click`      | float | 0.0–1.0       | 0.3     | Transient click intensity            |

## Acceptance criteria

- [ ] Module registered as `Kick` in `default_registry()`.
- [ ] Produces audible output when trigger goes high.
- [ ] `pitch` parameter controls fundamental — output pitch tracks the
      parameter.
- [ ] Pitch sweep is audible: higher `sweep` values produce a more
      pronounced downward chirp.
- [ ] `drive` parameter adds saturation — output clips harder at 1.0.
- [ ] Doc comment follows module documentation standard.
- [ ] Unit tests: trigger produces non-silent output, output decays to
      near-zero after decay time, pitch parameter affects output frequency.
- [ ] `cargo clippy` clean, no `unwrap()`/`expect()`.

## Notes

Depends on: 0327 (DecayEnvelope, PitchSweep, saturate)
Epic: E061
