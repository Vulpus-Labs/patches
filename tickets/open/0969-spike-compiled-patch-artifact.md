---
id: "0969"
title: Spike — compiled patch artifact (.patches → JSON) for player/CLAP load
priority: low
created: 2026-05-28
---

## Summary

Investigation only (not committed scope). Evaluate persisting the patch-graph
JSON as a "compiled" form of a `.patches` file + its includes, and having
`patches-player` / `patches-clap` load from it where present. Recorded as ADR
0079 Open question 1; parked here so the idea isn't lost. **Do not implement
without a follow-up decision.**

## Acceptance criteria

- [ ] Written finding (a few paragraphs, or a short ADR if it graduates)
      covering:
  - **Perf reality:** the JSON is pre-interpreter, so only parse+expand
    (~100µs/file per the pest bench) is skipped; interpret → ModuleGraph →
    SCC/fusion → buffer layout → engine build is **not**. Quantify whether
    there's any load-time win worth the machinery.
  - **Staleness model:** source vs compiled divergence — hash/mtime freshness
    gate, recompile-on-stale, or reject. Live-coding hot-reload wants source.
  - **Schema skew:** old compiled JSON + newer player → version field +
    compatibility handling.
  - **CLAP plugin-state self-containment** (the strongest pro): a DAW project
    embedding the compiled patch survives the source files moving. Assess
    against however CLAP state is/should be serialized.
- [ ] Recommendation: pursue (with a follow-up epic/ADR), or close as not worth
      it.

## Notes

- ADR 0079 Open question 1. Not part of E157 or E158.
- Default prior (from the ADR discussion): defer — weak perf case, hot-reload
  wants source, added staleness/versioning burden; only the CLAP state angle is
  compelling and it's a separate concern.
