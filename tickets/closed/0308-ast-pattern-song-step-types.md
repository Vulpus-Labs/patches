---
id: "0308"
title: "AST types for pattern blocks, song blocks, and steps"
priority: high
created: 2026-04-11
---

## Summary

Define the AST types in `patches-dsl/src/ast.rs` for the new `pattern`
and `song` top-level constructs, and extend the `File` struct to carry
them alongside templates and the patch block.

## Acceptance criteria

- [ ] `Step` struct with fields: `cv1: f32`, `cv2: f32`, `trigger: bool`,
      `gate: bool`, `cv1_end: Option<f32>`, `cv2_end: Option<f32>`,
      `repeat: u8`
- [ ] `PatternChannel` struct with `name: Ident` and `steps: Vec<Step>`
- [ ] `PatternDef` struct with `name: Ident`,
      `channels: Vec<PatternChannel>`, and `span: Span`
- [ ] `SongRow` struct with `patterns: Vec<Option<Ident>>` (None for `_`
      silence)
- [ ] `SongDef` struct with `name: Ident`, `channels: Vec<Ident>`,
      `rows: Vec<SongRow>`, `loop_point: Option<usize>`, and `span: Span`
- [ ] `File` struct gains `patterns: Vec<PatternDef>` and
      `songs: Vec<SongDef>` fields
- [ ] `cargo test -p patches-dsl` passes
- [ ] `cargo clippy -p patches-dsl` clean

## Notes

These are the AST types only — no parsing logic yet. The parser tickets
(0309–0312) populate these types from the grammar. The `Step` type here
is the parsed representation; the runtime `Step` in patches-core (ticket
0318) may differ slightly.

Epic: E057
ADR: 0029
