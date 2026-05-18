---
id: E150
title: patches-vintage internal dedup and tightening
status: closed
created: 2026-05-18
closed: 2026-05-18
---

## Goal

`patches-vintage` (in
[patches-bundles](https://github.com/Vulpus-Labs/patches-bundles)) grew
by copy-paste during the BBD/chorus/flanger/reverb sprint. The November
2025 code-smell sweep fixed the two real bugs (vflanger/vstereobbd
seed collision, `Tap` HP coefficient) and the easy hot-loop noise, but
left a pile of structural duplication: vbbd↔vstereobbd reimplement the
same per-tap chain twice; vflanger↔vflanger_stereo are near-verbatim
mono/stereo twins; the four ladder/VCF modules are 90% the same
parameterised by mono/poly + kernel. Three different triangle-LFO
implementations sit in three sibling modules.

This epic collapses the duplication, lifts shared primitives into a
single home inside `patches-vintage`, and ticks off the remaining
notable items from the sweep that don't fit cleanly into the dedup
tickets.

## Scope

- Per-tap BBD chain (compand → tanh → BBD → expand → tanh-feedback →
  filtered feedback) extracted from `vbbd` so `vstereobbd` reuses it
  for both channels instead of importing `Tap` and reimplementing the
  loop.
- `vflanger` core extracted so `vflanger_stereo` is a thin
  L+R wrapper around two cores rather than a parallel copy of
  `OnePoleLpf`, constants, and the whole BBD-flanger chain.
- `vladder` / `vpoly_ladder` / `vota_vcf` / `vota_poly_vcf` share one
  body parameterised over the filter kernel and the mono/poly port +
  CV shape.
- Local `patches-vintage/src/primitives.rs` (mirroring
  `patches-drums/src/primitives/`) hosts the shared triangle LFO and
  the one-pole HP/LP idioms currently re-coded per module.
- `bbd_proto` polish: split the 170-line `process` per arm, reject
  `(Dark, Both)` at bind time instead of silently coercing, clean up
  the misleading `normalised_pair_residues` hardcoded loop.
- FFI manifest test hardening (defensive nullptr / UTF-8 asserts
  around the `unsafe` blocks in `lib.rs`).
- Catch-all sweep for the remaining minor items the November pass
  flagged (see ticket 0913).

## Out of scope

- **Promoting shared primitives into `patches-dsp`**. That's a
  separate published crate with its own version cadence; for now the
  helpers live as `patches-vintage`-local primitives. If a second
  bundle (drums, fft) ends up wanting the same triangle LFO, lift
  then, not now.
- **Touching `vreverb`'s process loop**. The November sweep claimed
  vbbd/vstereobbd/vreverb shared a chain, but vreverb is structurally
  different (Hadamard 8-tap matrix reverb, no compander). Only
  vbbd/vstereobbd share the chain — vreverb stays as-is.
- **Drum or FFT module dedup**. This epic is patches-vintage only.
- **Behavioural changes**. All dedup is structural; goldens (when
  reconstituted per ticket 0890) must remain bit-identical to current
  output.
- **Modifying the `Bbd` / `BbdProto` public API**. Internal callsite
  refactors only.

## Tickets

- [0908 — Dedup vbbd + vstereobbd per-tap chain](../tickets/closed/0908-dedup-vbbd-vstereobbd-tap-chain.md) ✓
- [0909 — Dedup vflanger + vflanger_stereo into shared core](../tickets/closed/0909-dedup-vflanger-stereo-cores.md) ✓
- [0910 — Dedup ladder/VCF four-way over kernel + axis](../tickets/closed/0910-dedup-ladder-vcf-modules.md) ✓
- [0911 — Shared triangle LFO + one-pole primitives in patches-vintage](../tickets/closed/0911-shared-lfo-onepole-primitives.md) ✓
- [0912 — bbd_proto polish: split process, residues, Dark+Both bind rejection](../tickets/closed/0912-bbd-proto-polish.md) ✓
- [0913 — FFI manifest test hardening + minor smell sweep](../tickets/closed/0913-vintage-ffi-and-minor-sweep.md) ✓

## Acceptance

- `cargo test --workspace` in `patches-bundles` green across all
  vintage modules (descriptor shapes + DSP property tests unchanged).
- `cargo clippy --all-targets -- -D warnings` green.
- Total LOC in `patches-vintage/src/` reduced by a meaningful amount
  (rough target: −500 to −800 lines net) without losing tests or
  behavioural coverage.
- No duplicated `OnePoleLpf` / `OnePoleHpf` / triangle-LFO
  definitions remain in the crate; grep confirms a single home.
- Per-module seed-salt registry (`0xBBD0_xxxx` constants, the
  `instance_id ^ salt` pattern) lives in one place with a comment
  table listing every assignment.
- If ticket 0890 lands first or alongside, goldens stay
  bit-identical. If it lands later, property-test bounds (output
  energy, bounded peak, decay shape) hold for every refactored module.

## Notes

The November 2025 sweep already fixed the truly critical items:

- vflanger vs vstereobbd seed collision (`0xBBD0_0020` was reused on
  both; vflanger moved to `0xBBD0_0050`).
- `Tap::fb_hp_r` coefficient (`1.0 - TAU·fc/sr` Taylor approx →
  `exp(-TAU·fc/sr)`).
- vreverb `1.0/sqrt(N)` hoisted out of the audio loop.
- vchorus `Mode::Off` redundant `mode_table` arms collapsed.
- `bbd_proto::demo_input_residues` scaffolding comment cleaned up.
- vbbd delay-CV `±2.0` magic clamp named (`DELAY_CV_MIN/MAX`).

This epic is the structural follow-up: real bugs are gone, the work
that remains is duplication removal and last-mile tightening.
