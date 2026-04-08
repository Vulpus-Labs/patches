---
id: "0248"
title: Parse error location accuracy
priority: low
created: 2026-04-02
---

## Summary

Parse errors are currently tested only for existence (`is_err()`), never for the
accuracy of their reported source location. A parser that reports every error at
offset 0 would pass all existing tests.

## Acceptance criteria

- [ ] For at least 3 of the existing negative fixtures (`missing_arrow`,
      `malformed_index`, `unclosed_param_block`), assert that the `ParseError`
      span points to a byte offset within the erroneous token or its immediate
      vicinity (not at offset 0 or end-of-file).
- [ ] For `int_literal_overflow_returns_parse_error`, assert the span covers
      the overflowing literal.
- [ ] Document in a comment what "correct location" means for each case (the
      exact token or region).

## Notes

- `ParseError` has a `span: Span` field with `start` / `end` byte offsets.
  The tests should assert `span.start` falls within a reasonable range, not
  necessarily an exact byte offset (to avoid brittleness if whitespace changes).
- Epic: E046
