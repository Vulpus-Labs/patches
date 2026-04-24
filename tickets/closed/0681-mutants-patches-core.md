---
id: "0681"
title: Mutation testing — patches-core
priority: medium
created: 2026-04-24
epic: E117
---

## Summary

Run `cargo mutants -p patches-core`; triage survived mutants; identify
under-tested files.

## Acceptance criteria

- [ ] Run completes; counts recorded.
- [ ] Top-5 MISSED-ratio files listed.
- [ ] Follow-up test tickets filed for hotspots (linked here).
- [ ] Benign-mutant patterns noted (for epic rollup).

## Notes

Depends on 0680.
