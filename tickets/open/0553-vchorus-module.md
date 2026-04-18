---
id: "0553"
title: VChorus — Juno-60-style vintage chorus module
priority: medium
created: 2026-04-18
epic: E090
depends_on: ["0552"]
---

## Summary

Add `VChorus` to `patches-modules`: a stereo Juno-60-style chorus
built on the BBD core from ticket 0552. Shared triangle LFO, inverted
right-channel modulation, fixed mode presets (I, II, I+II), with a
small amount of modelled hiss for character.

## Design

Location: `patches-modules/src/vchorus.rs`.

### Signal path (per sample)

1. Mono input (sum of `in_left`/`in_right` if both connected; else
   whichever is connected).
2. Two `Bbd` instances (MN3009 preset), fed the same input.
3. Triangle LFO (shared). Left BBD delay = base + depth·lfo(t);
   right BBD delay = base - depth·lfo(t) (inverted). Strict
   mathematical triangle, linear ramps.
4. Inject hiss into wet path: white noise via xorshift, scaled, added
   to each BBD output before output summing. Gain calibrated so wet
   SNR lands ~55–65 dB.
5. Output: `out_left = dry + wet_L`, `out_right = dry + wet_R` at a
   fixed dry/wet ratio matching the Juno-60 summing-resistor balance
   (start 1:1, tune by ear against TAL-Chorus-LX).

### Mode table

| Mode | LFO Hz | Delay min | Delay max |
| ---- | ------ | --------- | --------- |
| I    | 0.513  | 1.66 ms   | 5.35 ms   |
| II   | 0.863  | 1.66 ms   | 5.35 ms   |
| I+II | 9.75   | 3.30 ms   | 3.70 ms   |

Mode enum values: `one`, `two`, `both`, `off` (off bypasses, dry only).

### Ports

- Inputs: `in_left`, `in_right` (mono), `rate_cv` (additive Hz offset),
  `depth_cv` (additive fraction, clamped).
- Outputs: `out_left`, `out_right` (mono).

### Parameters

| Name    | Type | Range                | Default | Description |
|---------|------|----------------------|---------|-------------|
| `mode`  | enum | `off`/`one`/`two`/`both` | `one` | Chorus mode |
| `hiss`  | float | 0.0 -- 1.0          | `1.0`   | Hiss amount (0 = silent, 1 = hardware-matched) |

No user mix control — Juno has none; hard-coded dry/wet.

### Implementation notes

- Noise source: reuse `patches_dsp::noise::xorshift64` (already present).
- LFO: local phase accumulator; generate triangle directly
  (no wavetable). Phase increments by `rate_hz / sample_rate`.
- `rate_cv` and `depth_cv` add to the mode-preset values, clamped to
  sane bounds so CV abuse can't yank delay out of the BBD buffer.
- Doc comment per CLAUDE.md module-doc standard (Inputs/Outputs/
  Parameters tables matching `ModuleDescriptor` strings).

## Acceptance criteria

- [ ] `patches-modules/src/vchorus.rs` implements the module with the
      above ports/params.
- [ ] Module registered in `patches-modules/src/lib.rs` and exposed
      through the default registry.
- [ ] DSL test: a `.patches` patch wiring a sine osc → VChorus →
      `audio_out` builds and runs without allocation on the audio
      thread.
- [ ] Integration test in `patches-integration-tests`: verify mode
      switch changes measured L/R cross-correlation (modes I/II:
      strong anti-correlation due to inverted LFO; mode `both`:
      tighter, shallower).
- [ ] Hiss is audible but not dominant at `hiss = 1.0`; silent at
      `hiss = 0.0`.
- [ ] Module doc comment follows the CLAUDE.md standard.
- [ ] `cargo clippy` and `cargo test` clean across workspace.

## Notes

A/B check: compare against TAL-Chorus-LX (free Juno-60 emulation) on
the same input. Parity not required, but the chorus should read as
recognisably Juno-60 — slow breathing stereo for I/II, fast tight
vibrato-ish stereo for I+II.

Modern digital chorus will be a separate module in a later epic.
