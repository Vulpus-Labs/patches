---
id: "0836"
title: Reduce `File` clones in DSL desugar passes
priority: medium
created: 2026-05-08
epic: E139
---

## Summary

DSL desugaring passes (`desugar_taps`, `host_control_desugar`) reconstruct
the entire `File` AST by cloning every field, even ones the pass doesn't
touch. Per file rebuild on LSP edit this allocates includes, templates,
patterns, songs, sections, and connections wholesale.

The hot path: every keystroke in a `.patches` file → tree-sitter reparse
→ pest parse → desugar passes → expand → bind → diagnostic publish. Any
clone-heavy step here shows up as latency.

## Sites

- [patches-dsl/src/desugar.rs:100](../../patches-dsl/src/desugar.rs#L100)
  — `File` cloned even when no taps to desugar (early-exit case still
  pays full clone cost).
- [patches-dsl/src/desugar.rs:230-237](../../patches-dsl/src/desugar.rs#L230)
  — `File` reconstructed by cloning every untouched field.
- [patches-dsl/src/host_control_desugar.rs:107-116](../../patches-dsl/src/host_control_desugar.rs#L107)
  — same shape; reconstructs `File` including untouched sections.
- [patches-dsl/src/expand/substitute.rs:94](../../patches-dsl/src/expand/substitute.rs#L94)
  — `resolved.clone()` inside arity expansion loop; hoist or take the
  last iteration.

## Acceptance criteria

- [ ] No-op desugar (file has no taps and no host controls) does not
      clone `File` fields — verify by inspection or by adding a counter
      in tests
- [ ] `desugar_taps` and `host_control_desugar` either consume `File`
      and return `File`, or take `&mut File`; intermediate clones live
      only on the modified branch (vec push, pattern rewrite, etc.)
- [ ] No behaviour change: `patches-dsl` tests pass; integration-tests
      pass
- [ ] Optional: bench LSP `did_change` round-trip on a 200-line patch
      before/after; report numbers in PR description
- [ ] `just commit -p patches-dsl` clean

## Notes

Approach options, in increasing-disruption order:

1. **Take `File` by value, return `File`**: cheapest and idiomatic.
   Mutate vecs in place via `Vec::extend` / `Vec::iter_mut`.
2. **`File::map_patches` consuming-builder helper**: factors the
   pattern when more passes are added.
3. **`Arc<[T]>` for rarely-mutated sections** (templates, includes):
   refcount-bump on reconstruction. Heavier change; only if benches
   show clone cost dominates.

Recommend (1) unless profiling argues otherwise.
