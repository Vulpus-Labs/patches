---
id: "E102"
title: Vintage Juno voice — DCO, VCF, exponential ADSR, demo patch
created: 2026-04-20
tickets: ["0601", "0602", "0603", "0604"]
---

## Goal

Round out `patches-vintage` with the remaining Juno-60/106 voice
components so a complete Juno-shaped signal path can be built in the
DSL end-to-end. Existing modules (`PolyHighpass`, `PolyVCA`,
`PolyADSR`, `PolyToMono`, `VChorus`, and the BBD delay/reverb work)
already cover the downstream and shared stages. Three new pieces plus
a demo patch finish the set.

## Scope

1. **VDco / VPolyDco** — vintage digitally-controlled oscillator,
   mono and poly variants over a shared DSP core. One phase
   accumulator drives saw, variable-width pulse, and sub (÷2 square).
   Noise summed in the same module via an internal mixer stage. All
   waveshapes phase-locked to a single phasor — the Juno DCO's
   defining feature.
2. **VVcf / VPolyVcf** — 4-pole ZDF ladder LPF, mono and poly
   variants over a shared kernel, with per-stage tanh saturation,
   self-oscillation, and a `variant: Juno60 | Juno106` switch
   selecting between crisper (IR3109) and softer (80017A) behaviour.
3. **ADSR / PolyADSR exponential mode** — add a `shape` parameter
   (`Linear` default, `Exponential` new) to both the mono `ADSR` and
   the `PolyADSR` modules. RC-style segment updates with
   attack-overshoot clamp, matching analog envelope character.
4. **`vintage_synth.patches`** — example patch wiring the full Juno
   signal path: `VDco → PolyHighpass → VVcf → PolyVCA → PolyToMono →
   VChorus → out`, with `PolyADSR` (exponential) modulating VCF
   cutoff and VCA gain, and a `PolyLFO` for PWM / cutoff mod. Serves
   as the integration test that everything composes.

## Non-goals

- Global panel HPF switch semantics — HPF is per-voice with a shared
  control, already covered by `PolyHighpass` fan-out.
- A vintage VCA — Juno VCA runs well inside OTA linear range;
  existing `PolyVCA` is audibly indistinguishable.
- Voice assignment / polyphony internals — the existing poly wrapper
  handles 6-voice allocation.
- DCO clock quantisation at high pitch (subtle tuning error); skip
  unless a later ticket calls for it.
- DCO reset glitch HF sparkle; not modelled.

## Trademark / naming policy

Same as E090: module names are generic (`VDco`, `VVcf`), not Roland
trademarks. Documentation may cite Juno-60 / Juno-106 as hardware
references under nominative fair use.

## Design notes

### Signal flow (per voice)

```text
VPolyDco (saw + pulse + sub + noise, mixed internally)
  → PolyHighpass (stepped 4-position, shared control)
  → VPolyVcf (per-stage tanh ladder, 60/106 variant)
  → PolyVCA
  → (poly-to-mono mix bus)
  → VChorus
  → out
```

The vintage-synth patch uses the poly variants; the mono `VDco` and
`VVcf` are there for non-polyphonic uses (drones, mono leads,
modulation sources).

### VDco / VPolyDco

- One phase accumulator per voice (lives in `patches-dsp`).
- Saw: raw phase, polyBLEP at wrap.
- Pulse: comparator on raw phase (`phase < pwm`), independent polyBLEP
  at *both* edges (threshold crossing + wrap). Never derive pulse from
  BLEP-corrected saw — that skews duty cycle.
- Sub: ÷2 flip-flop on phase-wrap event, independent polyBLEP at its
  own edges.
- Noise: internal white noise source (reuse `patches-dsp::noise`).
- Internal mixer: saw and pulse are on/off; sub and noise have level
  sliders. Gains biased, not equal: saw ≈ pulse ≈ 1.0, sub max ≈ 1.0,
  noise max ≈ 0.5. Worst-case sum ≈ 3.5× single source — sent hot into
  VCF on purpose. No saturator in the mixer.

### VVcf / VPolyVcf

- 4-pole ZDF ladder kernel in new `patches-dsp::ladder`.
- Per-stage tanh; self-oscillation at full resonance.
- Variant param selects coefficients: `Juno60` (IR3109-ish — crisper,
  more aggressive peak) vs `Juno106` (80017A-ish — softer resonance,
  slight HF loss, bass compresses with resonance).
- Inputs: `cutoff` (voct or Hz — match existing filter conventions),
  `resonance` 0..1, optional `drive` (expose tanh headroom).
- CV summing (env amount, LFO, key-track) happens upstream in the
  patch, not inside the module.

### ADSR / PolyADSR exponential mode

- Add `shape: enum { Linear, Exponential }` parameter to both `ADSR`
  and `PolyADSR` via the shared `patches-dsp::adsr` core.
- Exponential: `y += k * (target - y)` per sample, with per-segment
  time constant derived from the A/D/R slider values.
- Attack target = 1.2 × peak, clamped at peak (standard analog trick
  for usable near-linear audible portion). Constant, not a param.
- Decay target = sustain level. Release target = 0.
- State machine unchanged.

### Demo patch

- Save at `patches-vintage/patches/vintage_synth.patches` (or the
  conventional examples location — check existing vintage patches).
- 6-voice poly. Panel-shaped parameter set exposed at the top for
  live-coding.
- Include a short companion doc comment at the top of the file
  pointing at the Juno-shaped signal path and noting trademark
  disclaimer.

## Acceptance criteria

- [ ] `VDco`, `VPolyDco`, `VVcf`, `VPolyVcf` registered via
      `patches-vintage`.
- [ ] `ADSR` and `PolyADSR` both have `shape` parameter with `Linear`
      (default) and `Exponential` modes, round-tripping through DSL.
- [ ] `vintage_synth.patches` loads, plays, and hot-reloads in
      `patches-player`.
- [ ] Audio sanity: self-oscillating VVcf tracks pitch; mixing all
      four DCO sources hot into VVcf with resonance high produces
      clean OTA-style saturation, no hard-clip artefacts.
- [ ] Polyphony: 6 simultaneous voices run without allocation on
      audio thread (covered by existing allocator trap once Spike 4
      lands; otherwise manual verification).
- [ ] `cargo clippy` and `cargo test` clean across the workspace.

## Dependencies

- `patches-vintage` crate (E090).
- polyBLEP helper — check `patches-dsp` for existing implementation;
  if absent, add in the VDco ticket.
- Ladder filter kernel — new `patches-dsp::ladder` module.

## Out of scope, future work

- Vintage LFO variant (triangle + delay-to-onset envelope) if the
  existing `PolyLFO` does not already cover this shape. Deferred to
  a follow-up if the demo patch surfaces a gap.
- DSP-accurate DCO clock-quantisation tuning error at high pitch.
- Per-voice detuning / drift (explicitly not a Juno character).
