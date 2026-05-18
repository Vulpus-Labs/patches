---
id: E151
title: Native dynamics, stereo utility, and source-tree reorg
status: in-progress
created: 2026-05-18
---

## Goal

Close the two structural holes in the native (`patches-modules`) bundle
flagged by ADR 0076: no compressor/gate (only peak limiters), and no
stereo image utility set (no pan, balance, width, mid/side, or
low-frequency monoizer). Land the audio-to-trigger / audio-to-gate
detector family that ADR 0047's sub-sample sync events need on the
ingest side. Add the two missing primitives (`DcBlocker`, `Comb`). And
fold the source tree into the semantic group directories the ADR
specifies, so the flat-vs-grouped inconsistency stops growing.

All dynamics modules adopt one sidechain convention at introduction
(`sidechain` port, self-key fallback on unconnected). True-stereo
dynamics use one linked detector — no L/R-independent stereo "comps".
Sub-sample edge detectors interp at `threshold`, never at
`threshold - hysteresis`; hysteresis controls eligibility, not event
location.

## Scope

### New modules

- **Dynamics:** `Compressor`, `StereoCompressor`, `Gate`, `StereoGate`.
- **Audio→control detectors:** `AudioToTrigger` (mono / stereo / poly),
  `AudioToGate` (mono / stereo / poly).
- **Stereo utility:** `Pan`, `Balance`, `StereoWidth`, `MidSide`,
  `MonoBass`.
- **Primitives:** `DcBlocker` (thin wrapper over
  `patches_dsp::DcBlocker`), `Comb` (single module with
  `ff`/`fb`/`both` mode enum).

### Conventions adopted

- `sidechain` port name on all dynamics.
- Self-key fallback when `sidechain` is unconnected (read from `in`).
  Implemented via `MonoInput::is_connected()`, no `Option` on the
  audio path.
- Stereo dynamics use one detector: peak = `max(|L|, |R|)`,
  RMS = `sqrt((L² + R²) / 2)`. One gain-reduction / gate state
  applied to both channels.
- Edge detectors: rising/falling/both direction, hysteresis re-arm
  band, optional cooldown debounce. Sub-sample fraction
  `(threshold - prev) / (curr - prev)` reported in ADR 0047 form.
  `AudioToGate` reuses the state machine but emits sample-accurate
  gates per ADR 0030.

### DSP/module separation

Every new module follows the kernel-outside-the-struct pattern:

- DSP kernel lives in a group-local `common/` submodule
  (`dynamics/common/comp_detector.rs`, `detectors/common/edge.rs`) or
  in `patches-dsp` if a second consumer exists.
- Kernel tests are colocated with the kernel, plain Rust state +
  functions, no `ModuleHarness`.
- Module-surface tests cover only connectivity-dependent or
  enum-dependent behaviour (self-key fallback, peak vs RMS branch,
  `Comb` mode, `AudioToTrigger` direction, `MidSide` partial wiring,
  stereo-dynamics linked detector under asymmetric L/R transients).

### Source-tree reorganisation

`patches-modules/src/` gains the semantic group directories from
ADR 0076. One ticket per group directory; structural-only changes;
top-level `pub use` re-exports preserve every existing public path
so downstream crates need no edits. Target layout:

```text
patches-modules/src/
├── common/         (existing)
├── dynamics/       compressor, gate, limiter, transient_shaper, stereo variants
├── stereo/         pan, balance, width, mid_side, mono_bass, stereo_split, stereo_sum
├── detectors/      audio_to_trigger, audio_to_gate, trigger_sync_conv
├── filter/         (existing)
├── mixer/          (existing)
├── modulators/     adsr, lfo, sah, glide, op, quant, tuner, ring_mod (+ poly siblings)
├── osc/            oscillator, noise (+ poly siblings)
├── delay/          delay, stereo_delay
├── reverb/         fdn_reverb
├── effects/        bitcrusher, drive
├── midi/           midi_arp, midi_cc, midi_delay, midi_drumset, midi_source,
│                   midi_split, midi_to_cv, midi_transpose
├── sequencer/      master_sequencer, pattern_player, tracker_core
├── host/           host_control, host_transport, audio_in, audio_out,
│                   ms_ticker, tempo_sync, clock
├── utils/          sum, vca, tap, mono_to_poly, poly_to_mono, quant_util (+ poly siblings)
└── primitives/     dc_blocker, comb
```

## Out of scope

- **Parametric / shelving / graphic EQ.** Separate ADR (0077); the
  `filter/` group has a slot, nothing here blocks it.
- **Wavefolder as a sibling module.** `Drive`'s `fold` mode already
  covers it.
- **Expander.** Gate's binary semantics cover the common case; a
  ratio-based downward expander ships when a real use case appears.
- **Phaser.** Belongs in `patches-vintage`, not native.
- **`AudioIn` host channel expansion** for true outboard sidechain.
  E148 / future scope.
- **Multi-band dynamics.** Out of scope for native stdlib.
- **Behavioural changes to existing modules.** Reorg tickets are
  structural only — same `mod.rs` re-exports, same descriptor shapes,
  same tests beyond import-path updates.

## Tickets

New-module work (alphabetised inside each group; numbering assigned
sequentially as tickets are opened):

- 0914 — Sidechain convention + self-key reads (limiter proof-point
  or doc-only rationale)
- 0915 — `Compressor` + `StereoCompressor`
- 0916 — `Gate` + `StereoGate`
- 0917 — `AudioToTrigger` (mono / stereo / poly)
- 0918 — `AudioToGate` (mono / stereo / poly)
- 0919 — Stereo utility: `Pan`, `Balance`, `StereoWidth`, `MidSide`,
  `MonoBass`
- 0920 — Primitives: `DcBlocker`, `Comb`

Source-tree reorg (one per target group; existing group dirs that
are unchanged are not listed):

- 0921 — `dynamics/` group dir (move `limiter`, `stereo_limiter`,
  `transient_shaper`; land new comp/gate here)
- 0922 — `stereo/` group dir (move `stereo_split`, `stereo_sum`;
  land new pan/balance/width/midside/monobass here)
- 0923 — `detectors/` group dir (move `trigger_sync_conv`; land new
  audio-to-trigger / audio-to-gate here)
- 0924 — `modulators/` group dir (move adsr, poly_adsr, lfo,
  poly_lfo, sah, poly_sah, glide, op, poly_op, quant, poly_quant,
  tuner, poly_tuner, ring_mod)
- 0925 — `osc/` group dir (move oscillator, poly_osc, noise)
- 0926 — `delay/` group dir (move delay, stereo_delay)
- 0927 — `reverb/` group dir (rename `fdn_reverb/`)
- 0928 — `effects/` group dir (move bitcrusher, drive)
- 0929 — `midi/` group dir (move all `midi_*`)
- 0930 — `sequencer/` group dir (move master_sequencer,
  pattern_player, tracker_core)
- 0931 — `host/` group dir (move host_control, host_transport,
  audio_in, audio_out, ms_ticker, tempo_sync, clock)
- 0932 — `utils/` group dir (move sum, poly_sum, vca, poly_vca, tap,
  mono_to_poly, poly_to_mono, quant_util)
- 0933 — `primitives/` group dir (land new dc_blocker, comb here;
  may merge with 0920)

Docs:

- 0934 — Manual + module-reference docs update (cover every new
  module; reorg is invisible to docs)

## Acceptance

- `cargo test --workspace` green; new kernels carry property /
  golden tests, new modules carry connectivity / enum-branch surface
  tests as described in ADR 0076.
- `cargo clippy --all-targets -- -D warnings` green.
- `just push` green.
- No duplicate sidechain-self-key implementations across dynamics;
  one helper (or one inline pattern) shared.
- Stereo dynamics tests demonstrate image preservation: asymmetric
  L/R transient produces identical gain reduction / gate state on
  both channels (the regression-trap for "fixing" the linked
  detector back to per-channel).
- `MidSide` round-trip test: encode then decode within `ε` of input.
- Every existing public path in `patches-modules` continues to
  resolve (`patches_modules::Compressor`, `patches_modules::Pan`,
  `patches_modules::Oscillator`, `patches_modules::ResonantLowpass`,
  etc.); downstream crates compile without edits.
- Manual at `docs/src/modules/` covers every new module with the
  port / parameter table form mandated by `CLAUDE.md`.

## Notes

ADR 0076 is the source of truth. Where this epic and the ADR differ,
the ADR wins — file a corrective edit on this epic before diverging.

Reorg tickets must avoid drive-by behavioural changes. The temptation
to "while we're here, clean up X" inside a structural move is what
makes large reorgs review-hostile; resist it and open a follow-up
ticket if X is worth fixing.

The `Comb` module ships as one struct with a `mode` enum rather than
three modules (`CombFf`, `CombFb`, `CombBoth`). The enum branch is
the surface test; the kernel test covers the maths once per branch.
