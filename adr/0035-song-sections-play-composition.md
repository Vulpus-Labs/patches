# ADR 0035 — Song sections and play composition

**Date:** 2026-04-14
**Status:** accepted
**Supersedes parts of:** ADR 0029 (tracker-style pattern sequencer)

---

## Context

ADR 0029 introduced `song` blocks as pipe-delimited tables: a header row
of channel names followed by data rows of pattern references. The data
rows are the final arrangement — there is no way to factor out repeated
material or to compose a song from named parts.

Real arrangements have structure (verse, chorus, middle-eight). Authors
currently duplicate rows, which is tedious and error-prone, and editing
pipe-aligned tables is fussy.

## Decision

Replace the pipe-table song body with a composition-oriented syntax.

### Song header declares lanes

```patches
song foo(drum, bass, lead) {
    ...
}
```

The song's lane list replaces the pipe-table header row. Lanes are
declared once per song and are in scope for every row within it.

### Sections define reusable row sequences

```patches
section verse {
    pat1, bass_c, lead_c
    pat2, bass_f, lead_f
}
```

A row is a comma-separated list of cells (pattern name, `_` silence, or
`<param>` reference). Rows are separated by newlines — newlines are
significant inside row-sequence contexts.

A row group `( ... ) * N` repeats a sub-sequence:

```patches
section verse {
    (pat1, bass_c, lead_c
     pat2, bass_f, lead_f) * 2
    pat3, bass_g, lead_g
}
```

Row groups may nest. The repeat multiplier applies only to a
parenthesised group — `a, b, c * 2` (bare cell repeat) is rejected at
parse time.

### Play statements compose sections into the song

```patches
play verse
play (verse, chorus) * 4
play middle_8
play chorus * 2
```

Grammar:

```
play_stmt    = "play" play_body
play_body    = inline_block | named_inline | play_expr
play_expr    = play_term ("," play_term)*
play_term    = play_atom ("*" integer)?
play_atom    = ident | "(" play_expr ")"
inline_block = "{" row_seq "}"
named_inline = ident "{" row_seq "}"
```

`*` binds tighter than `,`. `a, b * 2` expands to `a, b, b`. Sections
referenced by `play` are resolved by name at expansion.

### Inline definitions

A simple song with no reusable parts can skip `section` entirely:

```patches
song foo(drum, bass, lead) {
    play {
        pat1, bass_a, lead_a
        pat1, bass_b, lead_a
    }
}
```

A named-inline form defines a section and plays it once. Subsequent
references replay it by name:

```patches
play chorus {
    pat3, bass_g, lead_g
    pat3, bass_f, lead_f
}
play verse
play chorus
play chorus
```

This mirrors the convention in lyric sheets: the chorus is written out
the first time, then later occurrences just say "chorus". A named-inline
is exactly equivalent to a top-of-song `section chorus { ... }` followed
by `play chorus` — it is sugar for the common case where the definition
naturally lives at first use.

The inline forms (`play { ... }` and `play foo { ... }`) appear only as
the entire body of a `play` statement, not as atoms inside a play
expression. To compose named sections, define them with `section` (or
introduce them via a prior `play foo { ... }`) and reference them in
`play foo, bar`.

### Loop marker

`@loop` is a standalone song item between `play` statements:

```patches
play intro
@loop
play verse, chorus
```

At most one `@loop` per song. When present, `loop_point` in the emitted
`Song` is the row index reached at the marker.

### Patterns may be defined inline

`pattern` blocks may appear as song items alongside `section` and `play`:

```patches
song foo(drum) {
    pattern fill { kick: x . x . x . x . }
    play verse { fill, }
}
```

Inline patterns are song-local (see Scoping below). Top-level `pattern`
blocks remain available.

### Sections may be defined outside songs

`section` blocks may appear at file top level. Top-level sections are
visible to all songs but have no intrinsic lane count — row widths are
validated against the invoking song's lanes at expansion. A section used
by two songs with different lane counts is an expansion error only at
the mismatched call site.

### Scoping

Scope layers, innermost first:

1. **Song** — song-local patterns and sections.
2. **Template** (if the song is nested in a template) — template params
   and template-local definitions.
3. **File** — top-level patterns, sections, templates.

Lookup walks the parent chain. Song-local definitions shadow outer names
within that song only; sibling songs never see each other's locals.
Templates that define songs inherit the template's scope as the song's
parent, so pattern refs in the song can resolve to template-injected
patterns.

Scoping relies on `QName` (ADR 0034): inline patterns are mangled by
extending `QName::path` with the enclosing song name. The emitted
`PatternBank` contains every pattern under a unique `QName`, and cell
references inside a song resolve to the scope-winning `QName`. The
interpreter's alphabetical bank-index assignment operates on
`Display`-formatted `QName`s and remains stable.

## Alternatives considered

### Keep pipe tables, add a separate `section` block

Retain the existing song body syntax and add named sub-blocks referenced
by `play`.

Rejected: the pipe alignment burden is the primary complaint. Keeping
two row syntaxes (pipes for song bodies, commas for sections) multiplies
grammar surface with no readability gain.

### Newline-insensitive rows with an explicit separator (`;`)

Rows separated by `;` within sections/play blocks.

Rejected: noisier than significant newlines, and authors naturally write
one row per line anyway.

### Cell-chunking by lane count, no row separator

Flat cell stream, chunked into rows by lane count at expansion.

Rejected: a single typo in cell count silently reflows rows until a
final mismatch error far from the actual mistake. Significant newlines
catch typos at the site.

### Inline blocks only as play atoms (mixed with composition)

Allowing `play a, { ... }, b` so that an inline block could appear as
one term among many in a play expression.

Rejected: the inline forms exist for two specific ergonomic cases —
trivial single-section songs (`play { ... }`) and lyric-sheet-style
define-on-first-use (`play chorus { ... }`). Mixing inline blocks into
the middle of a composition expression introduces a third position
where row literals can appear and complicates the grammar without
serving an authoring use case. Composition uses named sections.

### Global pattern namespace with inline patterns hoisted

Inline patterns would be hoisted to file-level and become globally
visible.

Rejected: two songs may reasonably each have a `fill` pattern that means
different things. Song-local scoping matches authors' mental model.

## Consequences

- Grammar, AST, and expander rewrite for `song` blocks. The interpreter
  contract (`FlatPatch` containing `Song { channels, order, loop_point }`
  plus a pattern bank) is preserved — the expander still flattens
  compositional syntax into the same row table.
- The pipe-table song body is removed. All existing fixtures and
  documentation using it must be rewritten.
- Pest grammar gains a newline-significant sub-mode inside row-sequence
  contexts. Elsewhere (module decls, connections, play expressions
  themselves) whitespace handling is unchanged.
- Depends on ADR 0034 (`QName`) for song-local pattern mangling.
- LSP updates: hover/go-to-definition for section names and
  (where applicable) inline pattern references.
- Manual updates in `docs/src/` to describe the new song syntax and
  deprecate the pipe-table form.
