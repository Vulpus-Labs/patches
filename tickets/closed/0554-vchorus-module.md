---
id: "0554"
title: VChorus module + registry hook into patches-modules
priority: medium
created: 2026-04-18
epic: E090
depends_on: ["0553"]
---

## Summary

Implement `VChorus` in `patches-vintage/src/vchorus.rs`: a stereo
BBD chorus built on the 0553 BBD core. Two voicings (`bright` / `dark`)
and a shared triangle LFO with inverted right-channel modulation.
Wire it into the default registry through
`patches_vintage::register`.

## Design

### Signal path (per sample)

1. Mono input (sum `in_left`/`in_right` if both connected; else
   whichever is connected).
2. Two `Bbd` instances (MN3009 preset), fed the same input.
3. Shared triangle LFO, strict mathematical triangle, linear ramps.
   Left BBD delay = base + depth·lfo(t); right BBD delay
   = base − depth·lfo(t) (inverted).
4. Hiss injection: white noise via `patches_dsp::noise::xorshift64`
   scaled into wet path; wet SNR lands ~55–65 dB at `hiss = 1.0` on
   the `bright` voicing.
5. Output: `out_left = dry + wet_L`, `out_right = dry + wet_R` at a
   fixed dry/wet ratio per voicing.

### Variant (`bright` vs `dark`)

`bright` and `dark` are descriptive voicing names (the epic cites
Juno-60 and Juno-106 as hardware references under nominative fair
use; public names must not use the trademarks).

- **Mode set**: `bright` has I, II, I+II; `dark` has I, II only.
- **Post-BBD LPF cutoff**: `bright` ~9 kHz, `dark` ~7 kHz.
- **Dry/wet**: `bright` ≈ 1:1.15 (wet hotter); `dark` ≈ 1:1.
- **"Off" behaviour**: `bright` fully bypasses; `dark` passes
  through BBD with LFO depth zero.
- **Hiss floor**: `dark` ~6–8 dB lower than `bright` at matched
  `hiss = 1.0`.

### Mode table

`bright`:

| Mode | LFO Hz | Delay min | Delay max |
| ---- | ------ | --------- | --------- |
| I    | 0.513  | 1.66 ms   | 5.35 ms   |
| II   | 0.863  | 1.66 ms   | 5.35 ms   |
| I+II | 9.75   | 3.30 ms   | 3.70 ms   |

`dark`:

| Mode | LFO Hz | Delay min | Delay max |
| ---- | ------ | --------- | --------- |
| I    | 0.5    | 1.66 ms   | 5.35 ms   |
| II   | 0.83   | 1.66 ms   | 5.35 ms   |

Selecting `both` on `dark` rejects at bind time (decide impl detail
during coding).

### Ports

- Inputs: `in_left`, `in_right` (mono), `rate_cv`, `depth_cv`.
- Outputs: `out_left`, `out_right` (mono).

### Parameters

| Name      | Type  | Range                    | Default  | Description                                 |
| --------- | ----- | ------------------------ | -------- | ------------------------------------------- |
| `variant` | enum  | `bright`/`dark`          | `bright` | Voicing                                     |
| `mode`    | enum  | `off`/`one`/`two`/`both` | `one`    | Chorus mode (`both` only valid on `bright`) |
| `hiss`    | float | 0.0 -- 1.0               | `1.0`    | Hiss amount                                 |

No user mix control.

### Registry hook

`patches_vintage::register(r)` calls `r.register::<VChorus>()`.
`patches-modules::default_registry()` invokes this at the end. No DSL
change — VChorus is available by name through the default registry
from first merge.

### Implementation notes

- LFO: local phase accumulator, triangle via `abs(2·phase − 1)·2 − 1`
  (or similar). No wavetable.
- `rate_cv`/`depth_cv` are additive offsets on preset values, clamped
  so CV abuse can't yank delay out of the BBD buffer.
- Doc comment per CLAUDE.md module-doc standard.

## Acceptance criteria

- [ ] `patches-vintage/src/vchorus.rs` implements the module.
- [ ] `patches_vintage::register` registers `VChorus`; consumed by
      `patches-modules::default_registry`.
- [ ] DSL test in `patches-integration-tests`: a `.patches` patch
      with sine osc → VChorus → `audio_out` builds and runs without
      audio-thread allocation.
- [ ] Integration test: mode switch changes L/R cross-correlation
      (modes I/II: strong anti-correlation; mode `both`: tighter,
      shallower).
- [ ] Hiss audible-not-dominant at `hiss = 1.0`; silent at `0.0`.
- [ ] Module doc comment per CLAUDE.md standard.
- [ ] `cargo clippy` and `cargo test` clean workspace-wide.

## Notes

A/B reference during development: TAL-Chorus-LX (emulation of the
`dark` reference hardware). Parity not required; target is
recognisable character — slow breathing stereo for I/II, fast tight
vibrato-ish stereo for I+II on `bright`.

Modern digital chorus: separate epic later.
