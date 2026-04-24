---
id: "0674"
title: CLAP UI spike writeup and backend comparison
priority: medium
created: 2026-04-24
---

## Summary

Compare `patches-clap-vizia` and `patches-clap-webview` across the
axes that matter for the long-term GUI choice. Produce a written
evaluation in `docs/notes/` (or similar) that a follow-up ADR can
cite. No decision made in this ticket — the writeup is input to the
ADR, not the ADR itself.

## Acceptance criteria

- [ ] Document covers, for each backend:
  - Iteration speed: time to add a new widget / tweak layout.
  - Memory footprint: RSS of one and four plugin instances in a host.
  - CPU cost: idle window, meters running, meters hidden.
  - Binary size: built `.clap` bundle size per platform.
  - Cross-platform status: what works on macOS / Windows / Linux,
    with known gotchas.
  - Code volume: LOC in plugin crate + assets.
  - Duplication vs `patches-plugin-common`: what was shared, what
    wasn't, and what should move into common before committing.
  - LLM-assisted iteration experience — subjective but record
    specifics where possible.
- [ ] Recommendation with explicit tradeoff summary. Recommendation
      may be "keep both", "drop vizia", "drop webview", or "neither
      — try X".
- [ ] List of open questions the ADR will need to resolve.

## Notes

Writeup lives under `docs/notes/` or similar scratch location — not
yet an ADR. If recommendation is to commit to one backend, the next
step is an ADR + deletion of the other crate. If "keep both" for
now, capture the maintenance cost honestly.
