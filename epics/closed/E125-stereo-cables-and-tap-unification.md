---
id: "E125"
title: Stereo cable kind and unified Tap module
created: 2026-04-27
adrs: ["0059", "0054", "0030", "0015"]
tickets: ["0735", "0736", "0737", "0744", "0738", "0739", "0740", "0741", "0742", "0743"]
---

## Goal

Land ADR 0059 end-to-end: `CableKind::Stereo`, mono→stereo broadcast
coercion, single stereo ports on stereo modules, unified `Tap` module
(retiring `TriggerTap`), source-order slot allocation with
`(tap_type, name)` identity, and `foo/left`/`foo/right` stereo
metering pairs.

## Scope

1. **Core types.** Add `CableKind::Stereo`, `StereoInput`,
   `StereoOutput`, descriptor builders. Type checker rules for the
   coercion table in ADR 0059 §2.
2. **Planner / cable builder.** Mono→stereo broadcast on cable
   construction; stereo cables back onto poly storage (lanes 0–1).
3. **Stereo modules migration.** `stereo_delay`, `convolution_reverb`,
   `fdn_reverb`, `stereo_limiter`, mixer stereo variants, `audio_in`,
   `audio_out` switch from `*_left`/`*_right` mono ports to a single
   stereo port. Tests and example patches updated.
4. **Tap unification.** Single `Tap` module with `mono_in` /
   `stereo_in` / `trigger_in` per channel. `TriggerTap` removed.
   `stereo_meter` tap type added. Slot allocation: stereo claims 2
   slots, mono/trigger claim 1, channel counter advances by width.
5. **Tap identity & ordering.** Identity is `(tap_type, name)`;
   ordering is source location. Update desugarer, manifest builder,
   observer slot resolution. Drop alphabetical sort.
6. **Stereo metering pair convention.** Manifest emits `foo/left`,
   `foo/right` for `stereo_meter` taps. Parser rejects user tap names
   containing `/`. UI groups by stem.
7. **Docs & LSP.** ADR cross-references, manual updates, hover for
   stereo ports and stereo_meter tap type, port-name convention update
   in CLAUDE.md.

## Tickets

- 0735 — `CableKind::Stereo` + `StereoInput`/`StereoOutput` ports
- 0736 — Mono→stereo broadcast coercion in cable builder
- 0737 — Migrate stereo modules to single stereo ports
- 0744 — `StereoSplitter` and `StereoJoiner` utility modules
- 0738 — Update example patches and integration tests for stereo ports
- 0739 — Unified `Tap` module; retire `TriggerTap`
- 0740 — Stereo-aware slot allocation (width 2 for stereo channels)
- 0741 — Tap identity `(tap_type, name)` and source-order slot mapping
- 0742 — `stereo_meter` tap type + `foo/left`/`foo/right` manifest
- 0743 — Docs, LSP hover, CLAUDE.md port-naming update

## Success criteria

- All existing integration tests pass after migration.
- Drum-machine example uses stereo cables for the master bus and a
  `stereo_meter(master)` that surfaces as one paired widget in the
  ratatui player.
- `~trigger_led(kick)` and `~meter(kick)` coexist in one patch.
- A patch wiring a stereo source into a mono input fails with a
  `CableKindMismatch` diagnostic at the cable.

## Sequencing

0735 → 0736 → 0737/0744 in parallel → 0738.
0739 → 0740 → 0741 → 0742.
0743 last.
