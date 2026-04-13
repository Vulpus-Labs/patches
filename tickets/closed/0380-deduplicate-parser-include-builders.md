---
id: "0380"
title: Deduplicate DSL parser include builders
priority: medium
created: 2026-04-13
---

## Summary

In `patches-dsl/src/parser.rs`, `parse` and `parse_include_file` contain
identical pest error-mapping closures (lines 137–146 vs 159–168).
`build_file` and `build_include_file` (lines 201–256) are also near-duplicates
with the same match arms and field construction, differing only in whether a
`patch` is expected.

## Acceptance criteria

- [ ] Extract shared pest error-to-`ParseError` conversion into a helper function
- [ ] Unify or factor `build_file` / `build_include_file` to eliminate duplicated match arms
- [ ] All existing parser tests pass
- [ ] No new clippy warnings

## Notes

One approach: have `build_include_file` call shared logic, with `build_file`
adding the `patch` extraction on top. Alternatively, unify `File` and
`IncludeFile` with `patch: Option<Patch>`.
