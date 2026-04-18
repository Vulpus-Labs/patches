---
id: "0535"
title: Split patches-dsp biquad/tests.rs by category
priority: low
created: 2026-04-17
epic: E090
---

## Summary

[patches-dsp/src/biquad/tests.rs](../../patches-dsp/src/biquad/tests.rs)
is 738 lines. Tests fall into three clusters: poly (multi-voice)
ramp/fanout/independence behaviour, analog-prototype coefficient
checks (`butterworth_*`), and frequency-response family tests
(`t1_*`, `t2_*`, ...).

## Acceptance criteria

- [ ] Convert to stub `src/biquad/tests.rs` declaring a submodule
      tree under `src/biquad/tests/`.
- [ ] Category split (final naming the ticket's call):
      - `poly.rs` — `set_static_fans_out_*`, `begin_ramp_*`,
        `tick_all_*`, `voices_are_independent`, `poly_*`
      - `analog_prototypes.rs` — `butterworth_lp`, `butterworth_hp`,
        `butterworth_bp`
      - `frequency_response.rs` — numbered `t1_*`, `t2_*` tests and
        their `h_magnitude` / `check_frequency_response` helpers
- [ ] Shared helpers in `tests/mod.rs` or `tests/support.rs`.
- [ ] `cargo test -p patches-dsp` passes with the same test count.
- [ ] `cargo build -p patches-dsp`, `cargo clippy` clean.

## Notes

E090. No test logic edits. Numbered `t1`/`t2`/... families are kept
together in one file to preserve their series intent.
