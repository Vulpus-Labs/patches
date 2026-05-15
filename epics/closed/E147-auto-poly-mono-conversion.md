---
id: E147
title: Auto poly/mono cable conversion for Audio
status: open
created: 2026-05-15
adr: 0074
---

## Goal

Insert synthetic `MonoToPoly` / `PolyToMono` modules at descriptor_bind
time when an edge crosses between `MonoLayout::Audio` and
`PolyLayout::Audio`, or from `PolyLayout::Audio` into a stereo input
(composes `PolyToMono` with the existing mono→stereo broadcast
coercion). Mirrors the existing `coalesce_fan_in` auto-Sum pattern;
fusion (ADR 0072) eliminates the synthetic-hop tick delay. Non-Audio
layouts and stereo→poly continue to be rejected. See
[ADR 0074](../../adr/0074-auto-poly-mono-conversion.md).

## Scope

- New pass in `patches-interpreter::descriptor_bind`, sibling to
  `coalesce_fan_in`, that detects accepted kind-mismatch edges and
  rewrites them through synthesised `MonoToPoly` / `PolyToMono`
  instances with `__autoconv_` naming prefix.
- Subsumes validation relaxation: pass runs before the final
  kind-mismatch check.
- Both target modules already exist in `patches-modules/`; no new
  module code.
- Add `QName::is_synthetic()` umbrella helper covering both
  `__autosum_` and `__autoconv_`.
- Sweep every surface-tool filter site (SVG, LSP, profiler) currently
  using `is_autosum()` onto `is_synthetic()` so `__autoconv_*`
  instances are hidden everywhere `__autosum_*` already is.
- Audio-integrity golden entries for both directions, with bit-identity
  vs. explicit `MonoToPoly` / `PolyToMono` patches.

## Out of scope

- Non-sum folds (average, channel-pick, weighted mix) — explicit
  modules stay in `patches-modules` and gain no new sugar.
- Trigger / Transport / MIDI layout conversions — explicitly rejected
  by ADR 0074.
- FFI ABI changes — synthetic modules are host-side, plugins see only
  the resulting ordinary ModuleGraph nodes.
- Removing `MonoToPoly` or `PolyToMono` from `patches-modules` — kept
  for explicit cases and as the synthesis target.

## Tickets

- [0892 — descriptor_bind kind-conversion pass for mono↔poly Audio](../../tickets/open/0892-mono-poly-audio-broadcast.md)
- [0894 — Surface-tool sweep: filter `__autoconv_` in SVG, LSP, profiling](../../tickets/open/0894-lsp-mono-poly-audio.md)
- [0895 — Audio integrity goldens for mono↔poly Audio conversion](../../tickets/open/0895-mono-poly-audio-goldens.md)

Docs deliberately omitted — pending a thorough rewrite of project
documentation; ADR 0074 itself is the durable record until then.

## Acceptance

- Both broadcast and sum-fold work end-to-end in `patch_player` on a
  representative patch.
- All four tiers (`inner` / `commit` / `push` / `smoke`) green.
- Golden corpus includes a poly→mono sum patch and a mono→poly
  broadcast patch; both bit-identical to their explicit-module
  equivalents and stable across runs.
- LSP no longer reports `CableKindMismatch` for the two new accepted
  combinations on the syntax corpus, and `__autoconv_*` instances are
  hidden from LSP views, SVG export, and profiler timing reports.
- No FFI ABI version bump.
