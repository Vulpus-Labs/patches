---
id: "0130"
title: Register all new filters and add end-to-end integration test
priority: medium
created: 2026-03-18
epic: "E024"
depends_on: ["0127", "0129"]
---

## Summary

Verify that all six filter modules (mono LP/HP/BP and poly LP/HP/BP) are
reachable through the default registry and that the poly filters work correctly
end-to-end inside a patch graph using `HeadlessEngine`. This is the final
quality gate for E024.

## Acceptance criteria

- [ ] All six module names are present in the default registry in
      `patches-modules/src/lib.rs`:
      `"Filter"`, `"Highpass"`, `"Bandpass"`, `"PolyLowpass"`, `"PolyHighpass"`,
      `"PolyBandpass"`.

- [ ] Integration test file
      `patches-integration-tests/tests/poly_filters.rs` contains:

  - **`poly_lowpass_passes_low_frequencies`**: build a graph with a poly source
    (all voices at the same 200 Hz sine), a `PolyLowpass` with cutoff 1000 Hz,
    and a poly probe; run 4096 samples via `HeadlessEngine`; assert all 16
    voice outputs have peak amplitude > 0.8.

  - **`poly_lowpass_attenuates_high_frequencies`**: same topology, source at
    8000 Hz, cutoff 1000 Hz; after settling, all 16 voices peak < 0.1.

  - **`poly_highpass_passes_high_frequencies`**: `PolyHighpass` with cutoff
    1000 Hz; source at 8000 Hz; all 16 voices peak > 0.8.

  - **`poly_bandpass_passes_center`**: `PolyBandpass` with center 1000 Hz,
    `bandwidth_q` 2.0; source at 1000 Hz; all 16 voices peak > 0.7.

  - **`poly_filters_survive_plan_reload`**: build a patch with `PolyLowpass`,
    run 100 samples, reload the identical patch (exercises the planner's
    instance-reuse path), run 100 more samples, assert no panics and that
    outputs remain non-NaN.

- [ ] `cargo test -p patches-integration-tests` passes including the new tests.
- [ ] `cargo build`, `cargo test`, `cargo clippy` pass across all crates with
      no warnings.

## Notes

Local `PolySource` and `PolyProbe` test helpers already exist in
`patches-integration-tests/src/lib.rs` from T-0120; re-use them here rather
than defining new ones.

The `poly_filters_survive_plan_reload` test is a smoke test only. It verifies
that the planner's module-reuse path does not corrupt filter state. It does not
need to assert specific output amplitudes — non-NaN, non-panic is sufficient.
