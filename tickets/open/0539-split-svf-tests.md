---
id: "0539"
title: Split patches-dsp svf/tests.rs by test family
priority: low
created: 2026-04-17
epic: E090
---

## Summary

[patches-dsp/src/svf/tests.rs](../../patches-dsp/src/svf/tests.rs) is
661 lines organised into numbered test families `t1_*`–`t7_*`:
impulse response, frequency response passbands, DC/Nyquist behaviour,
stability under high-resonance / ADSR FM sweeps, SNR vs f64 reference,
determinism.

## Acceptance criteria

- [ ] Convert to stub `src/svf/tests.rs` declaring a submodule tree
      under `src/svf/tests/`.
- [ ] Category split by family (final naming the ticket's call):
      - `impulse.rs` — `t1_*`
      - `frequency_response.rs` — `t2_*`
      - `dc_nyquist.rs` — `t3_*`
      - `stability.rs` — `t4_*`, `t5_*`
      - `quality.rs` — `t6_*` (SNR) + `t7_*` (determinism)
- [ ] Shared helpers (`make_kernel`, `db`, `measure_steady_state_amplitude`)
      in `tests/mod.rs` or `tests/support.rs`.
- [ ] `cargo test -p patches-dsp` passes with the same test count.
- [ ] `cargo build -p patches-dsp`, `cargo clippy` clean.

## Notes

E090. Numbered families preserve their series intent through the
filenames. No test logic edits.
