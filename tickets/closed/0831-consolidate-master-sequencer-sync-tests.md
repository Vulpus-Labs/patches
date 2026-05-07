---
id: "0831"
title: Consolidate master_sequencer sync-enum tests
priority: low
created: 2026-05-07
epic: E138
---

## Summary

`patches-modules/src/master_sequencer/tests.rs:52-100` has four tests
(`sync_auto_selects_host_when_hosted`, `sync_auto_selects_free_when_standalone`,
`sync_free_overrides_hosted`, `sync_host_overrides_standalone`) that each
build a sequencer and assert one boolean field of the resolved sync mode.

Replace with one parametric test over `(SyncMode, hosted, expected)` tuples.

## Acceptance criteria

- [ ] Four tests collapsed into one table-driven test
- [ ] Same coverage of the four (mode × hosted) cases
- [ ] `just inner -p patches-modules` green

## Notes

Core resolution logic lives in `patches-tracker-core`; if covered there,
the module-side test can shrink further to a single happy-path smoke.
