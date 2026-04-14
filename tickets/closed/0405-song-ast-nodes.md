---
id: "0405"
title: AST for sections, play composition, song items
priority: high
created: 2026-04-14
---

## Summary

Add AST types backing the grammar in 0404.

## Acceptance criteria

- [ ] `SongDef` fields: `name: Ident`, `lanes: Vec<Ident>`,
      `items: Vec<SongItem>`, `span: Span`. `rows` and `loop_point`
      removed from the AST (they belong to the expander's output).
- [ ] `enum SongItem { Section(SectionDef), Pattern(PatternDef),
      Play(PlayExpr), LoopMarker(Span) }`.
- [ ] `SectionDef { name: Ident, body: Vec<RowGroup>, span: Span }`.
- [ ] `enum RowGroup { Row(SongRow), Repeat { body: Vec<RowGroup>,
      count: u32, span: Span } }`.
- [ ] `enum PlayBody { Inline { body: Vec<RowGroup>, span: Span },
      NamedInline { name: Ident, body: Vec<RowGroup>, span: Span },
      Expr(PlayExpr) }`.
- [ ] `PlayExpr { terms: Vec<PlayTerm> }`,
      `PlayTerm { atom: PlayAtom, repeat: u32 }` (repeat defaults to 1),
      `enum PlayAtom { Ref(Ident), Group(Box<PlayExpr>) }`.
- [ ] `SongItem::Play` carries a `PlayBody`.
- [ ] `File` and `IncludeFile` gain `sections: Vec<SectionDef>` for
      top-level sections.
- [ ] Parser in `patches-dsl/src/parser.rs` builds these nodes from the
      new grammar rules; all parser unit tests updated.

## Notes

Depends on 0404.
