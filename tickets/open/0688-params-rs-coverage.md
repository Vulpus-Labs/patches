---
id: "0688"
title: params.rs — coverage gaps from mutation run
priority: low
created: 2026-04-24
epic: E117
---

## Summary

0681 flagged `patches-core/src/params.rs` with 21/42 (50%) survived
mutants.

## Acceptance criteria

- [ ] Review MISSED list in `mutants.out/missed.txt` for this file.
- [ ] Add targeted tests for each genuine gap (ignore benign
      `kind_name` / `Display` mutants).
- [ ] Re-run and record residual MISSED.

## Notes

Triage first — expect a mix of real gaps and display noise.
