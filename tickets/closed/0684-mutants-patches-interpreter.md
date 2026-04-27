---
id: "0684"
title: Mutation testing — patches-interpreter
priority: medium
created: 2026-04-24
epic: E117
---

## Summary

Run `cargo mutants -p patches-interpreter`; triage. Validation and
ModuleGraph construction paths. Expect benign mutants in error-message
branches.

## Acceptance criteria

- [ ] Run completes; counts recorded.
- [ ] Top-5 MISSED-ratio files listed.
- [ ] Follow-up tickets filed for hotspots.

## Notes

Depends on 0680.
