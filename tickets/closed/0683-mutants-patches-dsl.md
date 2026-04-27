---
id: "0683"
title: Mutation testing — patches-dsl
priority: medium
created: 2026-04-24
epic: E117
---

## Summary

Run `cargo mutants -p patches-dsl`; triage. Focus on parser actions and
template expander logic.

## Acceptance criteria

- [ ] Run completes; counts recorded.
- [ ] Top-5 MISSED-ratio files listed.
- [ ] Follow-up tickets filed for hotspots.
- [ ] Note grammar-level mutants that are unviable / benign.

## Notes

Depends on 0680.
