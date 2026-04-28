---
id: "0744"
title: StereoSplitter and StereoJoiner utility modules
priority: medium
created: 2026-04-27
---

## Summary

Add two zero-DSP utility modules in `patches-modules` that bridge
stereo and mono cables explicitly, since the type system rejects
implicit stereo→mono coercion (ADR 0059 §2).

- `StereoSplitter` — `in: stereo` → `out_left: mono`, `out_right: mono`.
- `StereoJoiner`   — `in_left: mono`, `in_right: mono` → `out: stereo`.

Pass-through plumbing only: no parameters, no state, no allocation.
`tick()` reads the stereo lanes and writes the two mono outputs (or
vice versa).

## Acceptance criteria

- [ ] Both modules registered and parseable in the DSL.
- [ ] Doc-comment tables in the standard form (CLAUDE.md "Module
      documentation standard").
- [ ] Unit tests confirm pass-through equality (splitter then joiner
      reproduces the input bit-exact; joiner then splitter likewise).
- [ ] Type checker rejects feeding splitter outputs back into a stereo
      input directly — user must go through the joiner.
- [ ] Mono→stereo broadcast (0736) still applies on the joiner's mono
      inputs (no, irrelevant — joiner inputs are mono; just confirm
      stereo→splitter and splitter→consumer paths are well-typed).

## Notes

ADR 0059 §3a. These are the only modules in the post-migration
codebase whose port names use `_left` / `_right`, by design — they
are *defined* by the asymmetry of their halves. Keep that exception
documented in CLAUDE.md (covered by 0743).
