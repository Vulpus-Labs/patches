---
id: "0204"
title: Audit DSP and module tests; produce alignment report
priority: high
created: 2026-03-30
---

## Summary

Review all existing DSP-related and module tests across the workspace.
Produce a written audit document and a prioritised work-item report describing
what is needed to bring the codebase into alignment with ADR 0022.

## Acceptance criteria

- [ ] Every test in `patches-dsp`, `patches-modules`, and
      `patches-integration-tests` that touches DSP behaviour is catalogued with:
      - crate and module name
      - which ADR 0022 testing technique(s) it uses (or "none / ad-hoc")
      - whether it belongs in `patches-dsp` (DSP correctness) or
        `patches-modules` / integration (protocol/routing correctness)
- [ ] DSP algorithms embedded in `patches-modules` (or elsewhere) that have no
      independent unit tests in `patches-dsp` are identified and listed.
- [ ] Module tests that are currently serving as the primary vehicle for DSP
      correctness verification are flagged.
- [ ] Gaps in ADR 0022 technique coverage are noted per algorithm
      (e.g. "biquad filter has no frequency-response test").
- [ ] A `docs/dsp-test-audit.md` file is committed with the catalogue and
      technique summary.
- [ ] A prioritised work-item list is appended to (or linked from) the audit
      doc, suitable for use as the basis of E039 ticket breakdown.
- [ ] No code changes required; this ticket is research and documentation only.

## Notes

Reference: ADR 0022 (`adr/0022-externalisation-of-dsp-logic.md`) and its
catalogue of testing techniques (impulse response, frequency response, DC/Nyquist
boundary, stability, linearity, SNR, determinism, edge-case, golden-file,
statistical).

The audit document should make it easy to answer, for each algorithm:
"Does it have the right tests, in the right place, using the right technique?"
