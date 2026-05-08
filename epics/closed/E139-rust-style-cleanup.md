---
id: E139
title: Rust style cleanup (style-survey 2026-05-08)
status: open
created: 2026-05-08
---

## Summary

Style survey of crates with commits in the last week (excluding
`patches-modules`, `patches-vintage`, `patches-fft`) surfaced a cluster of
small-to-medium quality issues: scattered `.unwrap()`/`.expect()` violations
of CLAUDE.md policy, wasteful whole-`File` clones in DSL desugaring,
implicit state encoded as bools/sentinels where a typed enum reads better,
weakly-typed FFI prepare boundary, and a handful of micro-smells.

None of the findings are correctness bugs — the code works. The aim is to
narrow the gap between "what the compiler enforces" and "what the
invariants actually are", and to reduce allocator pressure in DSL
re-expansion paths used by the LSP. RT-safety constraints in audio-thread
code are respected throughout.

Each ticket is independently shippable and can be sequenced freely.

## Tickets

- 0835 — sweep `.unwrap()`/`.expect()` in library code (policy enforcement)
- 0836 — reduce `File` clones in DSL desugar passes
- 0837 — typed enums for CLAP scope/spectrum mode round-trips
- 0838 — type the FFI `prepare` boundary (`PrepareResult`, `NonNull` handle)
- 0839 — bundle FFI parameter-frame setup behind `ValidatedParamFrame`
- 0840 — flatten `ParamConversionError` (move discriminant to `BindError.code`)
- 0841 — classify LSP cursor into a `CompletionContext` enum
- 0842 — misc small smells (PartialEq<&str>, Default for MidiEvent, gen
  wraparound, ancestor iter, token-offset Span newtype, name→index map)

## Notes

Survey notes captured in chat 2026-05-08. Findings respect RT-safety:
audio-callback `Option<_>` checks, mutex-poison `into_inner()` recovery,
and `*const AudioClock` ownership patterns were considered and excluded.

Suggested order if working serially:

1. 0835 (mechanical, unblocks future audits)
2. 0836 (LSP latency win)
3. 0838 → 0839 (related; 0839 lands on top of 0838)
4. 0837, 0840, 0841 in any order
5. 0842 last as a cleanup pass

Mark this epic done when all eight tickets are closed.
