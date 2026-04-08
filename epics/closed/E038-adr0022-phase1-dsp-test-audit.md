# E038 — ADR-0022 Phase 1: DSP & module test audit

## Goal

Establish a clear picture of the current state of DSP and module testing across
the workspace, and produce a report describing what work is needed to bring the
codebase into alignment with the norms set out in ADR 0022.

## Background

ADR 0022 establishes that DSP algorithms should live in `patches-dsp` and be
tested independently of the module harness. Module tests should focus on
protocol correctness (port wiring, parameter dispatch, connectivity lifecycle),
not numerical DSP behaviour.

Before embarking on migration work, we need to understand:

1. What DSP tests already exist, where they live, and which ADR 0022 testing
   techniques (impulse response, frequency response, stability, etc.) they use.
2. Which module tests are currently doing double-duty as DSP tests.
3. What DSP logic is embedded in modules and has not yet been extracted to
   `patches-dsp`.
4. Where test coverage gaps exist according to ADR 0022 standards.

## Deliverables

- A written summary (`docs/dsp-test-audit.md` or similar) cataloguing:
  - Existing DSP tests by crate, module, and ADR technique used.
  - Module tests that are testing DSP correctness rather than module protocol.
  - DSP algorithms embedded in modules without independent tests.
- A prioritised report of work items needed to achieve ADR 0022 compliance,
  which will form the input to E039 (Phase 2).

## Tickets

| # | Title |
|---|-------|
| T-0204 | Audit DSP and module tests; produce alignment report |
