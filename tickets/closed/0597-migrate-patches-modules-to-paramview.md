---
id: "0597"
title: Migrate `patches-modules` implementations to `ParamView` accessors
priority: high
created: 2026-04-20
depends_on: ["0596"]
---

## Summary

Mechanical migration of every `update_validated_parameters` impl
in [patches-modules/](../../patches-modules/) (~60 modules) from
`&ParameterMap` access to `&ParamView<'_>` accessors.

## Scope

- `params.get_scalar("name")` / `match ParameterValue::..` arms
  → `params.float("name")` / `params.int(...)` /
  `params.bool(...)` / `params.enum_variant(...).try_into::<E>()`.
- `params.get_scalar("name") => ParameterValue::FloatBuffer(arc)`
  → `params.buffer("name")` returning `Option<FloatBufferId>`,
  then resolve via the runtime's `FloatBuffer` `ArcTable` to the
  `&[f32]` the module currently caches.
- Drop any destructive-take patterns that still exist
  (none should after E096, but audit).
- Typed enums already generated via `params_enum!` (E096): keep;
  feed `enum_variant(key)` through the enum's `TryFrom<u32>`.

## Acceptance criteria

- [ ] Every module in `patches-modules/src/**/*.rs` compiles
      against the new trait signature.
- [ ] Behaviour unchanged: full `cargo test -p patches-modules`
      green.
- [ ] Integration-test golden sweeps (simple, poly_synth,
      fm_synth, fdn_reverb_synth, pad, pentatonic_sah,
      drum_machine, tracker_three_voices) produce bit-identical
      output vs pre-migration baseline.
- [ ] `cargo clippy -p patches-modules` clean.
- [ ] No `unwrap`/`expect` in migrated code.

## Notes

Buffer-id → `&[f32]` resolution on the audio thread must be a
pointer snapshot taken at plan-adoption time (the `ArcTable`
write-side lives on the control thread). Modules that hold a
buffer across calls hold the id + resolved slice together; the
next plan replaces both atomically.

## Non-goals

- Out-of-tree consumers (0598).
- File variant resolution (0599).
- Shadow oracle retirement (0600).
