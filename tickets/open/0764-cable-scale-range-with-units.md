---
id: "0764"
title: Cable scale syntax accepts ranges with units (uni / bi)
priority: medium
created: 2026-04-30
adrs: ["0062"]
---

## Summary

Today a cable scale carries a single value: `-[0.5]->`, `-[440Hz]->`,
or `-[<param>]->`. We need a range form so a cable can map a unipolar
or bipolar source into a meaningful endpoint range, with units:

```text
osc -[uni(C2, C4)]-> filter.cutoff
lfo -[bi(440Hz, 1000Hz)]-> osc.freq
```

- `uni(lo, hi)` — source `[0, 1]`; `0 → lo`, `1 → hi`.
- `bi(lo, hi)`  — source `[-1, 1]`; `-1 → lo`, `+1 → hi`,
  `0 → (lo + hi) / 2`.

Hard clip at the destination endpoints; out-of-source-range inputs
saturate. `lo > hi` inverts the mapping.

Each endpoint accepts the same forms as today's `scale_val`:
unit-suffixed numbers (`440Hz`, `-12dB`, `0.5s`), plain numbers, note
literals (`C4`, `A#3`), or `<param>` references.

## Acceptance criteria

- [ ] Grammar adds `uni(...)` / `bi(...)` to the cable-scale arrow
      without breaking existing single-value cables.
- [ ] Both endpoints share the same unit *family*; cross-family pairs
      are rejected with a clear error (e.g. `bi(440Hz, -12dB)`).
      Within the pitch family, note literals and Hz literals freely
      mix — `bi(C1, 2kHz)` is valid and both endpoints lower to v/oct.
- [ ] `<param>` references work in either or both endpoints; the
      lowered coefficients update on parameter change through the
      existing port-update path that already rewrites `scale`.
- [ ] Input port structs (`MonoInput`, `PolyInput`, `StereoInput` in
      `patches-core/src/cables/`) grow `offset: f32` and
      `clip: Option<(f32, f32)>`; `read` becomes
      `v * scale + offset` then optional `clamp`.
- [ ] Pure-scalar cables keep the existing fast path: `offset = 0`,
      `clip = None`. Verified by a microbench.
- [ ] Range and scalar segments compose across nested template
      boundaries per ADR 0062.
- [ ] LSP hover on a range-mapped cable shows the resolved endpoints.
- [ ] Docs: `docs/src/dsl-reference.md` cable-scale section updated
      with examples.

## Notes

Reference: [ADR 0062 — Cable range expressions](../../adr/0062-cable-range-expressions.md).
The ADR is the source of truth for semantics, lowering, and
runtime application. This ticket is the implementation tracker.

Mechanism summary (per ADR):

- Lowering is to a coefficient pair `(scale, offset)` plus an optional
  `clip` window, all stored on the input port. No synthetic mapper
  module per cable.
- `uni(lo, hi)` → `scale = hi - lo`, `offset = lo`,
  `clip = Some((min, max))`.
- `bi(lo, hi)`  → `scale = (hi - lo) / 2`, `offset = (hi + lo) / 2`,
  `clip = Some((min, max))`.
- Pitch family unifies notes + Hz as v/oct; linear-in-v/oct =
  exponential-in-Hz, so no `_log` form.
