---
id: "0742"
title: stereo_meter tap type and foo/left foo/right manifest pairing
priority: medium
created: 2026-04-27
---

## Summary

Add `stereo_meter` tap type. Wires to `Tap.stereo_in` (cable kind
`Stereo`). Manifest emits two scalar tracks named `foo/left` and
`foo/right` for a tap declared `~stereo_meter(foo)`. Reserve `/` in tap
names so user-supplied names cannot collide with the convention.

## Acceptance criteria

- [ ] `stereo_meter` parses as a tap type; rejects non-stereo cable
      sources with a `CableKindMismatch` diagnostic.
- [ ] Mono source feeding `stereo_meter` uses the broadcast coercion
      (0736); both meter bars read the same level.
- [ ] Parser rejects user tap names containing `/`.
- [ ] Manifest emits `{name}/left` and `{name}/right` `TapDescriptor`
      entries pointing at consecutive slots.
- [ ] Ratatui `patches-player` groups `*/left`/`*/right` into a single
      paired widget labelled by stem.
- [ ] Drum-machine example master bus uses `~stereo_meter(master)`.

## Notes

ADR 0059 §4, §7. UI grouping is observer-side only; audio path stays
unaware of the pairing. Compound forms involving `stereo_meter` are out
of scope here — keep compound taps mono-only as in ADR 0054 §1.
