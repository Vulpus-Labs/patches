---
id: "0348"
title: Upgrade GLOBAL_CLOCK to GLOBAL_TRANSPORT poly backplane
priority: high
created: 2026-04-12
---

## Summary

Upgrade backplane slot 8 from `GLOBAL_CLOCK` (mono, sample
counter only) to `GLOBAL_TRANSPORT` (poly), carrying both the
existing sample counter and host transport state. The processor
always writes lane 0 (sample counter). The CLAP plugin
additionally populates lanes 1-8 from `clap_event_transport`.
In standalone mode only lane 0 is written; the rest default
to 0.0.

## Poly lane layout

| Lane   | Signal         | Description                                      |
| ------ | -------------- | ------------------------------------------------ |
| 0      | sample_count   | Monotonic sample counter (was `GLOBAL_CLOCK`)    |
| 1      | playing        | 1.0 while playing, 0.0 stopped                   |
| 2      | tempo          | BPM as float                                     |
| 3      | beat           | Beat position (fractional)                       |
| 4      | bar            | Bar number                                       |
| 5      | beat_trigger   | Pulse (1.0 for one sample) on beat boundary      |
| 6      | bar_trigger    | Pulse (1.0 for one sample) on bar boundary       |
| 7      | tsig_num       | Time signature numerator                         |
| 8      | tsig_denom     | Time signature denominator                       |

## Acceptance criteria

- [ ] Rename `GLOBAL_CLOCK` to `GLOBAL_TRANSPORT` in
      `patches-core/src/cables.rs` (slot 8)
- [ ] Lane index constants defined (e.g.
      `TRANSPORT_SAMPLE_COUNT`, `TRANSPORT_PLAYING`,
      `TRANSPORT_TEMPO`, etc.)
- [ ] Processor writes `CableValue::Poly` with sample counter
      in lane 0 (replaces current mono write)
- [ ] Add `write_transport()` method to `PatchProcessor` for
      populating lanes 1-8
- [ ] CLAP plugin reads `clap_event_transport` and calls
      `write_transport()` before the sample loop
- [ ] CLAP plugin derives beat/bar triggers by detecting edge
      changes between process calls
- [ ] Update `GLOBAL_DRIFT` and all references to
      `GLOBAL_CLOCK` throughout codebase
- [ ] `AudioEnvironment` gains `hosted: bool` field; CLAP
      sets `true`, standalone and tests set `false`
- [ ] Standalone `patch_player` continues to work (only
      lane 0 populated)
- [ ] `cargo test` and `cargo clippy` pass

## Notes

- No module currently reads `GLOBAL_CLOCK`, so the mono-poly
  upgrade has no downstream breakage.
- CLAP provides transport via `clap_event_transport` struct
  with flags indicating which fields are valid. Only populate
  lanes whose corresponding flags are set; leave others at
  0.0.
- Beat/bar triggers require tracking previous beat/bar
  position to detect crossings.
- Lanes 9-15 are reserved for future use.
