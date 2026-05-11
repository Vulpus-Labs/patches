---
id: "0867"
title: Drop `is_autosum_name` free function; callers use `AUTOSUM_PREFIX`
priority: low
created: 2026-05-10
epic: E144
---

## Summary

0857 introduced three surfaces for the same predicate in
[patches-core/src/qname.rs](patches-core/src/qname.rs):

- `pub const AUTOSUM_PREFIX: &str = "__autosum_"`
- `pub fn is_autosum_name(name: &str) -> bool`
- `pub fn QName::is_autosum(&self) -> bool`

The free function exists for the profiler
([patches-profiling/src/timing_collector.rs](patches-profiling/src/timing_collector.rs)),
which holds `&str` rather than `&QName`. A direct
`name.starts_with(AUTOSUM_PREFIX)` is one identifier shorter than
`is_autosum_name(name)`, and removing the function eliminates a
duplicate name in the `patches-core` surface.

## Acceptance criteria

- [ ] `is_autosum_name` removed from `patches-core::qname`.
- [ ] `patches-profiling::timing_collector` calls
      `name.starts_with(AUTOSUM_PREFIX)` (importing the const) at the
      single use site.
- [ ] `QName::is_autosum` kept (it has no method-receiver analogue on
      `&str`).
- [ ] `just push` clean.
