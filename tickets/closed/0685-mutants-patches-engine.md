---
id: "0685"
title: Mutation testing — patches-engine (builder / plan)
priority: medium
created: 2026-04-24
epic: E117
---

## Summary

Run `cargo mutants -p patches-engine`; scope to builder and execution
plan. Exclude CPAL integration and audio-thread hot path via file
globs (mutations there are hard to observe from tests and risk
timeouts).

## Acceptance criteria

- [ ] Run completes; counts recorded.
- [ ] Exclude list for CPAL / audio-thread files documented.
- [ ] Top-5 MISSED-ratio files listed.
- [ ] Follow-up tickets filed for hotspots in builder / plan.

## Notes

Depends on 0680.
