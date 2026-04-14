---
id: "0404"
title: Grammar for song lanes, sections, play expressions
priority: high
created: 2026-04-14
---

## Summary

Rewrite Pest grammar rules for `song` blocks per ADR 0035: lane
declaration, `section` blocks, `play` expression syntax (comma, `*`,
parens, named/anonymous inline blocks), row groups with repetition,
`@loop` marker, inline `pattern` blocks in songs, top-level `section`
blocks.

## Acceptance criteria

- [ ] `song_block` = `"song" ident "(" ident ("," ident)* ")" "{"
      song_item* "}"`.
- [ ] `song_item` = `section_def | play_stmt | loop_marker |
      pattern_block`.
- [ ] `section_def` accepted at file top level (`file` / `include_file`
      rules) and inside songs.
- [ ] `play_stmt` grammar — inline forms appear only as the whole body
      of a `play` statement, not as atoms inside a composition
      expression:

    ```text
    play_stmt    = "play" play_body
    play_body    = inline_block | named_inline | play_expr
    play_expr    = play_term ("," play_term)*
    play_term    = play_atom ("*" integer)?
    play_atom    = ident | "(" play_expr ")"
    inline_block = "{" row_seq "}"
    named_inline = ident "{" row_seq "}"
    ```

- [ ] `row_seq` is newline-significant: rows separated by one or more
      newlines; cells within a row separated by `,`; row groups
      `"(" row_seq ")" "*" integer` may nest.
- [ ] Bare-cell `* N` is a parse error (only row groups may be
      repeated).
- [ ] `loop_marker` = `"@loop"` (standalone).
- [ ] Pipe-table song body (`song_header_row`, `song_data_row`) rules
      removed.
- [ ] Parser unit tests exercising each construct, including nested
      groups, top-level sections, anonymous and named-inline play
      bodies, and rejection of inline blocks as atoms inside a
      composition expression (e.g. `play a, { ... }`).

## Notes

Pest `WHITESPACE` must exclude `\n` inside row-sequence contexts; use a
dedicated silent rule for the inline whitespace permitted between cells,
and consume newlines explicitly as row separators. Review how this
interacts with existing whitespace handling elsewhere in the grammar.
