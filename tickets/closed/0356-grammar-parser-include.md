---
id: "0356"
title: Grammar and parser support for include directives
priority: high
created: 2026-04-12
epic: "E067"
adr: "0032"
---

## Summary

Extend the pest grammar and parser to recognise `include "path"` directives in `.patches` files. Add a second root rule (`include_file`) for library files that contain only templates, patterns, songs, and further includes — no `patch {}` block.

## Design

**Grammar additions** (`patches-dsl/src/grammar.pest`):

```pest
include_directive = { "include" ~ string_lit }

include_file = { SOI ~ (include_directive | template | pattern_block | song_block)* ~ EOI }
```

Update the existing `file` rule to allow `include_directive` alongside `template`, `pattern_block`, and `song_block`.

**AST additions** (`patches-dsl/src/ast.rs`):

- `IncludeDirective { path: String, span: Span }` — a parsed include.
- Add `includes: Vec<IncludeDirective>` field to the existing `File` struct.
- `IncludeFile { includes, templates, patterns, songs, span }` — parsed library file without a `patch` block.

**Parser additions** (`patches-dsl/src/parser.rs`):

- `build_include_directive(pair) -> IncludeDirective`
- Handle `Rule::include_directive` in `build_file`, collecting into `file.includes`.
- `pub fn parse_include_file(src: &str) -> Result<IncludeFile, ParseError>` using `Rule::include_file`.

**Public API** (`patches-dsl/src/lib.rs`):

- Re-export `IncludeDirective`, `IncludeFile`, and `parse_include_file`.

## Acceptance criteria

- [ ] `include "foo.patches"` parses in both `file` and `include_file` rules
- [ ] `File.includes` populated with parsed directives (path string + span)
- [ ] `parse_include_file` succeeds on files without `patch {}`
- [ ] `parse_include_file` fails on files containing `patch {}`
- [ ] `parse` (master) still requires exactly one `patch {}` block
- [ ] `include` without a string literal is a parse error
- [ ] Existing tests continue to pass (includes are optional)
- [ ] `cargo test -p patches-dsl` and `cargo clippy` pass

## Notes

- The `include` keyword does not conflict with existing grammar because `include` is not currently a reserved word or module type name.
- The string in `include "..."` uses the existing `string_lit` rule (quoted, no escape sequences).
- This ticket only adds parsing; file resolution is ticket 0357.
