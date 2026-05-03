---
id: "0789c"
title: Migrate patches-vintage modules to TEMPLATE const
priority: medium
created: 2026-05-02
epic: "E132"
adrs: ["0066"]
depends_on: ["0786"]
---

## Summary

Migrate every module in `patches-vintage` to declare a static
`ModuleDescriptorTemplate` via `Module::template()` and drop the
imperative `describe(shape)` override. Parallels tickets 0787/0788/0789
for `patches-modules`; the vintage crate was missed when those tickets
were scoped. Required before phase 2 of ticket 0790 (deletion of
`Module::describe` from the trait).

## Scope

Twelve modules:

- `vchorus`, `vflanger`, `vflanger_stereo`
- `vladder`, `vpoly_ladder`
- `vota_vcf`, `vota_poly_vcf`
- `vdco` (mono + poly variant in `vdco/poly.rs`)
- `vbbd`, `vstereobbd` (multi-channel — use per-axis port templates)
- `vreverb`

The two BBD modules use `mono_in_multi("delay_cv", n)` etc. — they
need `per_axis_inputs` paired with `AxisId::CHANNELS`, and structural
parameters declared via `structural_params` / `per_axis_*_params` as
appropriate. Audit each against the `patches-modules` migrations
landed in 0788 (channel-aware) for the right shape.

## Acceptance criteria

- [ ] Each vintage module declares `const TEMPLATE` and the matching
      `fn template()` override.
- [ ] Existing `fn describe(shape: &ModuleShape) -> ModuleDescriptor`
      override removed from each `impl Module`.
- [ ] Descriptor output byte-identical pre/post migration (compare
      via existing tests; add a snapshot if none exists).
- [ ] In-crate tests calling `VFoo::describe(&shape)` switched to
      `VFoo::template().build_channels(shape.channels as u32)` — or
      kept on the trait default impl while `Module::describe` still
      exists.
- [ ] `cargo test -p patches-vintage` passes.
- [ ] `cargo clippy -p patches-vintage --all-targets` clean.

## Notes

- Pattern reference: any migrated module in `patches-modules` (e.g.
  `adsr.rs`, multi-channel example via 0788's tickets).
- BBD modules: `delay_cv`, `gain_cv`, `fb_cv` are per-channel CV
  inputs; per-channel `delay_ms`, `gain`, `feedback` parameters are
  per-axis realtime params.
- Once landed, add `0789c` to `epics/open/E132-static-descriptor-templates.md`
  in the `tickets:` list and the `Sequencing` block (parallel with
  0787/0787a/0787b/0788/0789).
- Unblocks phase 2 of ticket 0790 (trait-method removal) jointly with
  the FFI track (0795/0796).
