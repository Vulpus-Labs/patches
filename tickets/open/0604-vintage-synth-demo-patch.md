---
id: "0604"
title: vintage_synth.patches — Juno-shaped demo patch
priority: medium
created: 2026-04-20
epic: E102
depends_on: ["0601", "0602", "0603"]
---

## Summary

Ship a `.patches` file that wires the full Juno-shaped voice from the
new `VPolyDco`, `VVcf`, and exponential `PolyADSR`, through existing
`PolyHighpass` / `PolyVCA` / `PolyToMono` / `VChorus`. Serves as the
integration demo for E102 and as a regression fixture for the
vintage crate.

## Signal path

```text
VPolyDco → PolyHighpass → VPolyVcf → PolyVCA → PolyToMono → VChorus → out
        (shared ctl)      ^           ^
                          |           |
                 PolyADSR (exp) ──────┴── VCF cutoff mod + VCA gain
                 PolyLFO ─ PWM, cutoff mod
```

- 6-voice poly.
- Single exponential `PolyADSR` feeds both VCF cutoff (via an amount
  with invert) and VCA gain — Juno fingerprint.
- `PolyLFO` routed to DCO PWM and VCF cutoff with per-destination
  amount.
- Panel-shaped parameter surface exposed at the top of the patch
  for live-coding (cutoff, resonance, env amount, LFO rate/amount,
  mixer balance, chorus enable).

## Location

- Save at `patches-vintage/patches/vintage_synth.patches` (or match
  whichever directory existing vintage examples live in — check the
  tree before placing).

## Acceptance criteria

- [ ] Loads and plays in `patches-player`.
- [ ] Hot-reload works: editing the file while playing re-adopts the
      plan without dropouts.
- [ ] All 6 voices run; audible poly allocation on held chords.
- [ ] Chorus audibly present and bypassable from panel params.
- [ ] VVcf self-oscillation reachable at full resonance.
- [ ] Header comment names the signal path and cites Juno-60 / -106
      as hardware references under nominative fair use (E090
      convention).
- [ ] No allocation on audio thread across a 10 000-sample run
      (manual or allocator-trap verification).

## Notes

- Trademark: file name is `vintage_synth`, not `juno`. Use generic
  language in the header comment.
- If `PolyLFO` lacks the triangle + delay-to-onset shape for an
  authentic Juno LFO, document the gap in the epic and either
  proceed with the closest existing shape or open a follow-up
  ticket.
