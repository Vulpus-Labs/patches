---
id: "0920"
title: Primitives — DcBlocker, Comb
priority: medium
created: 2026-05-18
epic: E151
---

## Summary

Two missing primitives:

- `DcBlocker` — thin wrapper over `patches_dsp::DcBlocker`. No
  parameters; the DSP kernel's fixed cutoff stays put.
- `Comb` — single module covering feed-forward, feedback, and
  combined comb topologies via a `mode` enum (`ff` / `fb` / `both`).
  Parameters: `delay_ms`, `feedback` (only meaningful when mode
  includes `fb`), `mix`.

Land both in the new `primitives/` group dir (ticket 0933). If 0933
is split out, this ticket lands the module files into the flat layout
first and 0933 moves them; if 0933 lands first, the modules land
directly in `primitives/`.

## Acceptance criteria

- [ ] `DcBlocker` and `Comb` registered; descriptors match ADR 0076.
- [ ] `DcBlocker`: passthrough verified on a steady-DC input
      (output → 0 within the kernel's documented settling time).
- [ ] `Comb` `ff` mode: surface test confirms output is the
      analytical `mix · (in + g · delay(in))` for known delay /
      feedback / mix.
- [ ] `Comb` `fb` mode: surface test for stable feedback at a known
      delay/feedback combination; instability at `feedback >= 1` is
      not asserted (caller's problem).
- [ ] `Comb` `both` mode: combined transfer function verified
      against analytical reference.
- [ ] Surface test: mode enum dispatch — same parameters, different
      mode produces different output.
- [ ] Manual pages added under `docs/src/modules/`.
- [ ] `just commit -p patches-modules` green.

## Notes

The mode enum keeps the registry footprint at one module instead of
three. Internally the process loop branches once per tick on the
enum; branch-predictor cost is negligible. If a future profile
shows otherwise, split into three `Process` impls dispatched at
build time — but only then.
