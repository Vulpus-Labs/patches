---
id: "E119"
title: Tap observation bringup — engine, observer, ratatui player (meter only)
created: 2026-04-26
tickets: ["0699", "0700", "0701", "0702", "0703", "0704", "0705"]
adrs: ["0043", "0053", "0054", "0055"]
---

## Goal

Land the runtime half of the observation architecture end-to-end:
backplane, frame ring, observer thread, manifest plumbing, and a
ratatui `patches-player` skin that subscribes to the observer and
renders meter bars. The DSL surface shipped in E118; phase 2 makes
those tap declarations actually do something.

Scope per ADR 0055: **`meter` taps only**. Other tap types (`osc`,
`spectrum`, `gate_led`, `trigger_led`) parse but their observer
pipelines are stubbed with a "not yet implemented" diagnostic so
unsupported declarations don't silently no-op. Compound taps that
include unimplemented components fall under the same stub policy.

## Scope

1. **Backplane + AudioTap module.** `[f32; MAX_TAPS]` per ADR 0053 §4
   (`MAX_TAPS = 32`). `AudioTap::tick` writes
   `backplane[slot_offset[i]] = inputs[i]` per channel. No allocation,
   no branch on tap type. `TriggerTap` lands as a stub for symmetry.
2. **SPSC frame ring.** Audio → observer ring sized per ADR 0053 §5.
   Per-block write-chunk + commit + drop-on-full path on the audio
   side. Drop counter per slot.
3. **`patches-observation` crate.** Consumer end of the ring plus
   per-slot pipeline state and a subscriber surface (atomic-scalar
   "latest values" per ADR 0053 §7). Initial pipeline: `meter` (fused
   max-abs with ballistic decay + rolling-window RMS). Sample-rate
   aware. No engine, audio, or UI deps.
4. **Manifest plumbing.** Planner ships `Vec<TapDescriptor>` (E118
   ticket 0697 emitted shape) to the observer over the planner→
   observer control ring (ADR 0053 §6), with `sample_rate` injected at
   build time. Observer keys per-slot state by tap name; on slot
   shifts a one-frame meter blip is acceptable.
5. **Registry wire-up.** Register `AudioTap` / `TriggerTap` in the
   module registry so binds for patches with taps succeed. Closes the
   bind-broken state from E118 §0697's note.
6. **ratatui patches-player skin.** Replace current CLI status output:
   header (patch path, sample rate, oversampling, engine state),
   meter pane (one peak+RMS bar pair per declared meter tap, labelled
   by name, dB-coloured), event log pane, footer keybindings. Keys:
   `r` start/stop recording (existing `wav_recorder`), `q` quit. No
   live reload / module rescan / oversampling change in this bringup.
7. **TUI subscription.** Subscribe to observer latest-scalar surface,
   poll ~30 Hz. Per-tap drop counters surface in the event log.

## Tickets

- 0699 — Backplane + AudioTap/TriggerTap audio-side modules
- 0700 — SPSC frame ring (audio → observer)
- 0701 — patches-observation crate: consumer, meter pipeline,
  subscriber surface
- 0702 — Manifest plumbing: planner → observer control ring
- 0703 — Register tap modules; bind round-trip green
- 0704 — patches-player ratatui skin (panes, layout, input)
- 0705 — TUI subscribes to observer; drop counters in event log

## Out of scope

- Oscilloscope, spectrum, gate_led, trigger_led pipelines (stubbed
  with "not yet implemented"; later ADR/epic).
- Live controller surface (load patch, module rescan, oversampling
  toggle). Existing player reload stays as-is.
- CLAP plugin rewrite. Gated on this epic + controller surface
  stabilising. Tracked under ADR 0055's reference-frontend principle.
- SVG patch-graph rendering in the TUI (explicitly not pursued).
- Manifest crate relocation. Type stays in `patches-dsl` until a
  non-DSL producer exists.

## Definition of done

- A `.patches` file containing one or more `~meter(...)` taps loads,
  binds, runs, and produces visible peak+RMS bars in the player TUI.
- Per-tap drop counters increment when the audio side outpaces the
  observer (forced via slow-observer test fixture).
- Engine still hits real-time targets: no allocation on the audio
  path, no blocking, no missed deadlines under nominal load.
- Closing this epic unblocks the post-meter pipelines (osc / spectrum
  / LED) and the controller-surface ADR.

## Cross-references

- ADR 0043 — original tap observation framing (superseded in detail).
- ADR 0053 — three-thread split, backplane, ring, `MAX_TAPS = 32`.
- ADR 0054 — DSL surface, implicit tap modules, manifest, pipelines.
- ADR 0055 — bringup sequence; this epic is steps 2–6.
- E118 — phase 1 (DSL surface, validation, desugaring, LSP).
