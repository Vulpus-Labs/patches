---
id: "0894"
title: Surface-tool sweep — filter `__autoconv_` in SVG, LSP, profiling
priority: low
created: 2026-05-15
epic: E147
adr: 0074
---

## Summary

After ticket 0892 inserts synthetic `__autoconv_*` modules for accepted
mono↔poly Audio conversions and adds a `QName::is_synthetic()` umbrella
helper, sweep every surface-tool filter site that currently calls
`is_autosum()` onto `is_synthetic()` so `__autoconv_*` instances are
hidden the same way `__autosum_*` instances already are. Also stop
the LSP from emitting `CableKindMismatch` for the now-accepted
combinations.

## Acceptance criteria

- [ ] **SVG generator**: every call site in
      [patches-svg/src/flat_to_layout.rs](../../patches-svg/src/flat_to_layout.rs)
      (lines 55, 76, 81, 117 in the current tree) uses
      `is_synthetic()` instead of `is_autosum()`. SVG export of a
      patch with auto-converted edges shows neither the synthetic
      `MonoToPoly` / `PolyToMono` nodes nor the extra hops — the
      visible graph is identical to the user's wiring.
- [ ] **Profiling**: timing collector filter in
      [patches-profiling/src/timing_collector.rs](../../patches-profiling/src/timing_collector.rs)
      (lines 93, 118) uses `is_synthetic()`; auto-converted patches
      produce timing reports that omit the synthetic instances and
      attribute no time to them.
- [ ] **LSP diagnostics**: pipeline accepts `MonoLayout::Audio ↔
      PolyLayout::Audio` without emitting `CableKindMismatch`. Likely
      automatic once 0892 lands in the interpreter, but verify and
      remove any duplicate gate in the LSP adapter layer.
- [ ] **LSP surface filters**: document symbols, hover, references,
      definition providers all use `is_synthetic()` and hide
      `__autoconv_*` instances. Audit search:
      `grep -rn "is_autosum\|AUTOSUM_PREFIX" patches-lsp/`.
- [ ] Other layout-mismatch combinations still surface as diagnostics
      with unchanged message text and severity.
- [ ] Syntax corpus in `patches-lsp/tests/syntax_corpus/` gains an
      entry exercising mono→poly Audio and one exercising poly→mono
      Audio; expected diagnostic sets are empty for the new
      connections and `__autoconv_*` nodes don't appear in expected
      symbol lists.
- [ ] SVG round-trip test: a patch with auto-conversion and a hand-
      wired equivalent produce visually identical SVG output (modulo
      whatever's intentionally different, e.g. node names if shown).
- [ ] `just inner -p patches-lsp -p patches-svg -p patches-profiling`
      green.

## Notes

The audit aims to be complete — once `is_synthetic()` exists, every
call site of `is_autosum()` in surface-tool code (SVG, LSP, profiling)
should switch. Direct uses of `AUTOSUM_PREFIX` constant should be
audited too, but those are typically in synthesis code (where keeping
the specific prefix is correct) rather than filter code.

Per syntax-corpus policy memory: corpus entries required for grammar
changes; this isn't a grammar change but corpus coverage is the
cheapest way to lock in the LSP behaviour.
