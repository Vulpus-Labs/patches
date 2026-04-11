---
id: "0309"
title: "Grammar + parser: step notation"
priority: high
created: 2026-04-11
---

## Summary

Add pest grammar rules and parser logic for the step notation used inside
pattern channel rows. This is the atomic unit of pattern data — each step
produces cv1, cv2, trigger, and gate values.

## Acceptance criteria

- [ ] Grammar rule `step` matching all step syntaxes from ADR 0029
- [ ] Note literals: `C4`, `Eb3`, `F#5` etc. — parsed to v/oct float
- [ ] Trigger shorthand: `x` parsed as cv1=0.0, trigger=true, gate=true
- [ ] Rest: `.` parsed as cv1=0.0, cv2=0.0, trigger=false, gate=false
- [ ] Tie: `~` parsed with trigger=false, gate=true, prev cv values
- [ ] Float literals: `0.5` parsed as cv1=0.5, trigger=true, gate=true
- [ ] Unit suffixes: `440Hz`, `2kHz`, `−6dB` etc. resolved at parse time
- [ ] cv2 via colon separator: `C4:0.8`, `x:0.7`, `0.5:0.3`
- [ ] Slides via `>`: `C4>E4`, `C4>E4:0.5>0.8`, `C4:0.5>0.8`
- [ ] Repeat via `*n`: `x*3`, `C4*2:0.8`
- [ ] Parser produces `Step` AST nodes (from ticket 0308)
- [ ] Unit tests for each step syntax variant
- [ ] `cargo test -p patches-dsl` passes
- [ ] `cargo clippy -p patches-dsl` clean

## Notes

Step notation is self-contained — no lookahead into adjacent steps is
required. The `~` (tie) carries over previous cv values; the parser sets
cv1/cv2 to a sentinel or default and the runtime handles carry-over.

The v/oct conversion for note literals: `voct = octave + semitone / 12.0`
where C0 = 0.0. This matches the existing `voct` convention used by
oscillator modules.

Epic: E057
ADR: 0029
