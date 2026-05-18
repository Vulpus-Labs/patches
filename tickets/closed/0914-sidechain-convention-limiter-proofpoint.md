---
id: "0914"
title: Sidechain convention + self-key reads (limiter proof-point)
priority: medium
created: 2026-05-18
epic: E151
---

## Summary

Adopt the ADR 0076 sidechain convention before any compressor / gate
code lands. `sidechain` is the port name. When unconnected, the
detector reads from `in` instead (self-key). Implementation reads
both each tick and selects via `MonoInput::is_connected()` — no
`Option`-typed cable read on the audio path.

Apply the convention to the existing `Limiter` as a proof-point: the
limiter does not gain new behaviour from a sidechain port, but adding
one (with self-key fallback that is the *current* behaviour) lets the
convention be exercised on tree before any new module depends on it.
Alternative: leave the limiter alone and instead land a doc note in
the ADR / a `common/` helper explaining why the limiter is exempt.
Pick one; do not leave the choice implicit.

For stereo dynamics the sidechain port is `Stereo` and the
ADR 0059 mono→stereo broadcast carries a mono sidechain source.

## Decision

**Limiter is exempt.** A peak limiter exists to bound the output by
`threshold`; a sidechain-driven detector decouples the keyed source from
`in`, so the dry path can exceed `threshold` whenever they diverge. That
breaks the contract the limiter is named for. Ducking and gating belong
on `Compressor` / `Gate`, which the convention is designed for.

The shared self-key helper lands now, in
`patches-modules/src/common/sidechain.rs`, so 0915 (compressor) and 0916
(gate) pick it up verbatim. `Limiter` and `StereoLimiter` carry a
one-paragraph exemption note in their module docs referencing this
decision.

## Acceptance criteria

- [x] Decision recorded above: limiter exempt; helper lands here.
- [x] Exemption notes in `Limiter` and `StereoLimiter` module docs.
- [x] Shared helper `mono_key` / `stereo_key` lives at
      `patches-modules/src/common/sidechain.rs` with unit tests.
- [x] Connectivity check is `is_connected()` boolean (helper takes a
      bool; comp/gate will pass `port.is_connected()` directly).
- [x] `cargo clippy --all-targets -- -D warnings` green (verified
      per inner-loop validation).

## Notes

This ticket pre-dates 0915/0916 deliberately. Comp and gate are easier
to land once the convention is exercised on real code and the helper
exists. The helper landed here so 0915 can pick it up.

The surface tests originally listed (unconnected vs explicit-self;
distinct source) belong on `Compressor` (0915) where they verify
audible behaviour; they don't apply once the limiter is exempt. The
helper unit tests in `common/sidechain.rs` cover the select logic
in isolation.
