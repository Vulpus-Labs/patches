---
id: "0447"
title: Stage-boundary newtypes and pipeline::parse cleanup
priority: low
created: 2026-04-15
---

## Summary

Pipeline stage boundaries are documented but not type-enforced:

- `crate::parse` returns `File`, which can still contain template
  definitions and unresolved references.
- After `expand`, the same `File` shape is meaningless — you have a
  `FlatPatch` instead. But nothing prevents a consumer from handing a
  pre-expansion `File` to a stage that expects post-expansion data.
- `pipeline::parse()` (`patches-dsl/src/pipeline.rs:43`) is a
  pass-through documented as "exists so callers can name the stage".
  No consumer calls it.

Introduce newtype wrappers that make stage discipline visible in
signatures:

- `ParsedFile(pub File)` — output of stage 1/2.
- `ExpandedPatch(pub FlatPatch)` — output of stage 3 (might re-use
  `FlatPatch` directly if it's already stage-specific).

Delete or promote `pipeline::parse()`: either remove it (it's dead
surface) or make it a real stage entry point that consumers must go
through.

## Acceptance criteria

- [ ] Stage-boundary newtypes (or equivalent type distinctions) added
      where they meaningfully prevent category errors. Don't add them
      decoratively — each new type must gate at least one compile-time
      check.
- [ ] `pipeline::parse()` is either deleted or given a distinct
      signature from `load` (e.g. `parse(src: &str) -> ParsedFile`).
- [ ] Consumer-facing API of `pipeline` documents which newtype each
      stage consumes and produces.
- [ ] `cargo test`, `cargo clippy` clean.

## Notes

Part of E082. Exploratory — if the newtype wrappers end up being
cosmetic rather than enforcing anything real, close the ticket with
a note explaining why and limit the scope to deleting `parse()`.
