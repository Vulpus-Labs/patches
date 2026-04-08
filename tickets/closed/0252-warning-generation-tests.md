---
id: "0252"
title: Warning generation tests
priority: low
created: 2026-04-02
---

## Summary

The expander returns `ExpandResult { patch, warnings }`, and the `Warning` type
is part of the public API, but existing tests only verify warnings are *absent*.
No test verifies that a warning is *produced* when expected. This means the
warning code paths are either dead code or completely untested.

## Acceptance criteria

- [ ] Identify at least one scenario that the expander is intended to warn
      about (read the expand.rs source for `Warning` construction sites).
- [ ] Write a test that triggers the warning and asserts it is present in the
      result with the expected message content.
- [ ] If no warning-producing code paths exist, document this finding and
      either remove the `Warning` type or add a TODO for when warnings should
      be emitted.

## Notes

- Possible warning candidates: unused template parameters, shadowed module
  names, scale of exactly 0.0, connections to/from the same module.
- Epic: E046
