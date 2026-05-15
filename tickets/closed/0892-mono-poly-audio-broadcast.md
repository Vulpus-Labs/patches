---
id: "0892"
title: descriptor_bind kind-conversion pass for mono↔poly Audio
priority: medium
created: 2026-05-15
epic: E147
adr: 0074
---

## Summary

Add a new pass in `patches-interpreter::descriptor_bind`, sibling to
`coalesce_fan_in`, that inserts synthetic `MonoToPoly` / `PolyToMono`
modules at edges where the source and target cable kinds differ in any
of the sugar-accepted combinations:

| Source kind          | Target kind          | Synthetic module |
| -------------------- | -------------------- | ---------------- |
| `MonoLayout::Audio`  | `PolyLayout::Audio`  | `MonoToPoly`     |
| `PolyLayout::Audio`  | `MonoLayout::Audio`  | `PolyToMono`     |
| `PolyLayout::Audio`  | `CableKind::Stereo`  | `PolyToMono`     |

The third row composes with the existing mono→stereo broadcast (ADR
0059 §2): a `PolyToMono` is inserted, leaving Mono Audio → Stereo
which the runtime ModuleGraph already broadcasts.

This subsumes the validation relaxation — the pass runs before the
final kind-mismatch check, converts accepted mismatches into legal
connections via synthetic modules, and leaves all other mismatches
(Trigger, Transport, MIDI on either side; stereo→poly) to be rejected
by the existing path. See
[ADR 0074](../../adr/0074-auto-poly-mono-conversion.md).

## Acceptance criteria

- [ ] New module (e.g. `descriptor_bind/kind_conv.rs`) implements the
      pass, structured to match
      [patches-interpreter/src/descriptor_bind/fan_in.rs:37-164](../../patches-interpreter/src/descriptor_bind/fan_in.rs#L37-L164).
- [ ] Pass called from [patches-interpreter/src/descriptor_bind/mod.rs:211](../../patches-interpreter/src/descriptor_bind/mod.rs#L211)
      area, after fan-in coalescing and before the final kind-mismatch
      validation.
- [ ] Synthetic instances named with `__autoconv_` prefix; constant
      `AUTOCONV_PREFIX` added alongside `AUTOSUM_PREFIX` in
      [patches-core/src/qname.rs:15](../../patches-core/src/qname.rs#L15)
      with corresponding `is_autoconv()` helper.
- [ ] Umbrella `QName::is_synthetic()` helper added that returns
      `is_autosum() || is_autoconv()`. This is the predicate that
      surface tools (SVG, LSP, profiler) should use going forward;
      0894 sweeps existing call sites onto it.
- [ ] Naming format mirrors auto-Sum: `__autoconv_<target>_<port>` or
      `__autoconv_<target>_<port>_<idx>` for indexed ports.
- [ ] Connections rewritten: source → synthetic, synthetic → original
      target. Synthetic module added to the bound module list.
- [ ] Module registry lookups for `MonoToPoly` and `PolyToMono` use the
      existing registered names; no shape parameter needed (both are
      fixed-width 1↔16 modules).
- [ ] `MonoLayout::Audio → PolyLayout::Audio` produces a `MonoToPoly`
      synthetic; `PolyLayout::Audio → MonoLayout::Audio` produces a
      `PolyToMono` synthetic; `PolyLayout::Audio → CableKind::Stereo`
      produces a `PolyToMono` synthetic whose Mono Audio → Stereo
      output edge then takes the broadcast coercion at runtime
      ModuleGraph construction; all three end-to-end via
      `build_from_bound`.
- [ ] All other kind mismatches continue to raise `CableKindMismatch`
      unchanged (Trigger/Transport/MIDI on either side, stereo→poly,
      stereo→mono — verify with unit tests).
- [ ] Unit tests in `patches-interpreter` cover: accepted conversions
      produce the expected synthetic node by name; rejected
      combinations still error; an existing fan-in test that mixes
      kinds (if any) still behaves correctly.
- [ ] Fusion verification: a patch with `mono_audio → poly_audio`
      via `__autoconv_*` produces an `ExecutionPlan` where both
      hops are fused (zero added tick delay). Add an explicit test if
      the planner already exposes fused-cable inspection; otherwise
      rely on golden-equality with a hand-wired equivalent patch.
- [ ] `just inner -p patches-interpreter -p patches-core -p patches-planner` green.

## Notes

The two modules already exist:

- [patches-modules/src/mono_to_poly.rs](../../patches-modules/src/mono_to_poly.rs)
- [patches-modules/src/poly_to_mono.rs](../../patches-modules/src/poly_to_mono.rs)

So no new module work — only the synthesis pass and the prefix plumbing.

Sequencing relative to `coalesce_fan_in`: fan-in coalescing must run
first so this pass sees one source per target edge. If fan-in produces
a Sum/PolySum/StereoSum and the synthesised sum's output kind mismatches
the original target, this pass picks that up the same way it would for
a user-declared instance. Verify with a fan-in-into-mono-input test
where the sources are poly Audio.

Per the syntax-corpus policy memory, no grammar change is implied — no
corpus entry strictly required, but a fixture patch under
`patches-interpreter/src/tests/fixtures/` exercising both directions
is cheap insurance.
