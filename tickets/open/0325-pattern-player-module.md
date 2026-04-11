---
id: "0325"
title: "PatternPlayer module"
priority: high
created: 2026-04-11
---

## Summary

Implement the `PatternPlayer` module in patches-modules. It reads a poly
clock bus, steps through pattern data from `TrackerData`, and outputs
cv1/cv2/trigger/gate signals per channel.

## Acceptance criteria

- [ ] Module registered as `PatternPlayer` in the module registry
- [ ] Shape arg: `channels` (int, with optional aliases)
- [ ] Poly input: `clock` (receives the MasterSequencer's clock bus)
- [ ] Mono outputs per channel (via `mono_out_multi`): `cv1`, `cv2`,
      `trigger`, `gate`
- [ ] Implements `ReceivesTrackerData` — receives full pattern bank
- [ ] On tick trigger (clock voice 2): advance step, read pattern data
      from bank index (clock voice 1), output step values
- [ ] On pattern reset (clock voice 0): reset step counter to 0
- [ ] Slides: interpolate cv1/cv2 from start to end over tick duration
      (clock voice 3)
- [ ] Repeats: subdivide tick into `n` evenly-spaced triggers using tick
      duration
- [ ] Tie (`~`): gate stays high, no trigger, cv carries over
- [ ] Rest (`.`): gate off, no trigger
- [ ] Channel count mismatch handled gracefully: excess pattern channels
      ignored, surplus player channels silent
- [ ] Stop sentinel (bank index -1): clear all gates, cease output
- [ ] Connected-port optimisation: unused output channels skip processing
- [ ] Doc comment follows module documentation standard
- [ ] Unit tests: basic step playback, slides, repeats, ties, rests,
      pattern switching, channel mismatch, stop sentinel
- [ ] `cargo test -p patches-modules` passes
- [ ] `cargo clippy -p patches-modules` clean

## Notes

The PatternPlayer is generic — it doesn't know whether its channels are
notes, drums, or automation. All channels produce the same four output
types. The wiring in the patch block determines how outputs are used.

Slide interpolation uses the tick duration from clock voice 3, so swing
is automatically accounted for without any swing-awareness in the player.

Epic: E060
ADR: 0029
