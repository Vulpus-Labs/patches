# ADR 0076 — Native dynamics and stereo utility modules

## Status

Proposed (2026-05-18)

## Context

The native (`patches-modules`) bundle covers oscillators, filters,
ADSR/LFO, delay/reverb, drive, limiter, transient shaper, and the
mixer/utility set. Two gaps are now blocking real patch work:

1. **Dynamics.** The bundle ships peak limiters but no compressor and
   no gate. Both are stdlib-grade primitives, and the limiter alone
   is not a substitute for either. Side-chained ducking and gating
   are common patch idioms.
2. **Stereo utility.** Stereo image control is missing — no pan, no
   balance, no width, no mid/side, no low-frequency monoizer. The
   `mixer` modules handle channel pan but do not cover the rest, and
   composing these by hand from `StereoSplit` / `StereoSum` is verbose
   and error-prone.

A third, smaller gap: there is no native way to derive a trigger or
gate from an audio signal. ADR 0047 sub-sample sync events are
supported on the cable side and exposed via the existing
`TriggerToSync` / `SyncToTrigger` converters, but those only handle
sample-accurate `0/1` pulses on the input. Treating an audio
oscillator's output as a clock, or building an envelope follower's
gating logic from a kick drum, currently requires extra modules and
careful threshold patching.

These gaps share design questions that benefit from one ADR:
sidechain convention, true-stereo detector linking, sub-sample edge
location under hysteresis, and the organisational shape of the
growing `patches-modules` source tree.

### Existing dynamics in tree

- [`limiter`](../patches-modules/src/limiter.rs) — mono lookahead peak
  limiter.
- [`stereo_limiter`](../patches-modules/src/stereo_limiter.rs) — already
  uses linked detection (`max(|L|, |R|)`) internally. The pattern
  proposed here generalises that decision to the comp/gate pair.
- [`transient_shaper`](../patches-modules/src/transient_shaper.rs) —
  attack/sustain envelope shaper. Belongs in the dynamics group.

### Existing source-tree shape

`patches-modules/src/` is a mix of flat `.rs` files (one module per
file) and submodule directories (`mixer/`, `filter/`, `fdn_reverb/`,
`master_sequencer/`, `pattern_player/`, `tracker_core/`, `common/`).
Mono/poly/stereo variants follow two conventions:

- **Sibling files** at the top level: `lfo.rs` + `poly_lfo.rs`,
  `filter/` directory + `poly_filter/` directory, `sum.rs` +
  `poly_sum.rs` + `stereo_sum.rs`, etc.
- **Subfiles** within one module directory: `mixer/mono.rs` +
  `mixer/stereo.rs` + `mixer/poly.rs` + `mixer/stereo_poly.rs`.

The `mixer/` convention is the cleaner one: variants of the same
conceptual module live together, share a `mod.rs` doc block, and the
public re-exports collect in one place. The flat sibling pattern is
the older one and obscures conceptual grouping (`poly_filter` is
filtering, not its own concept).

## Decision

### New modules

#### Dynamics group

| Module             | Ports                                            | Notes                                                                                                                                          |
| ------------------ | ------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `Compressor`       | `in: mono`, `sidechain: mono`, `out: mono`       | Feed-forward; peak/RMS detect (enum); soft knee with `knee_width`                                                                              |
| `StereoCompressor` | `in: stereo`, `sidechain: stereo`, `out: stereo` | True-stereo: one detector fed by `max(abs(L), abs(R))` (peak) or `sqrt((L² + R²) / 2)` (RMS); single gain reduction applied to both channels   |
| `Gate`             | `in: mono`, `sidechain: mono`, `out: mono`       | Single threshold + hysteresis; attack/hold/release                                                                                             |
| `StereoGate`       | `in: stereo`, `sidechain: stereo`, `out: stereo` | Linked detector as above; single gate state drives both channels                                                                               |

Compressor parameters: `threshold` (dB), `ratio` (1..∞), `knee_width`
(dB; `0` collapses to hard knee), `attack` (ms), `release` (ms),
`makeup` (dB), `detect` (`peak | rms`), `mix` (dry/wet).

Gate parameters: `threshold` (dB), `hysteresis` (dB), `attack` (ms),
`hold` (ms), `release` (ms). No `mix` — gating is binary in intent.

No poly variants. Per-voice dynamics inside a poly bus are typically
handled by per-voice VCAs and ADSRs; channel-strip dynamics are
mono/stereo.

#### Audio-to-control detectors

| Module                 | Ports                                       | Notes                                                                                           |
| ---------------------- | ------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| `AudioToTrigger`       | `in: mono`, `out: trigger`                  | Rising-edge detect with sub-sample interp; threshold + hysteresis + direction + cooldown params |
| `PolyAudioToTrigger`   | `in: poly`, `out: poly trigger`             | Independent detector per channel                                                                |
| `AudioToGate`          | `in: mono`, `out: mono` (gate convention)   | Sustained gate while `in > threshold` (with hysteresis)                                         |
| `PolyAudioToGate`      | `in: poly`, `out: poly` (gate convention)   | Per-channel gate state                                                                          |

**No stereo variants in either family.** The trigger family threshold-
crosses the **instantaneous signed sample** (this is what makes the
sub-sample interp formula `frac = (threshold - prev) / (curr - prev)`
well-defined). The gate family — `AudioToGate` — uses the same signed
comparison (`signal > threshold`) so its mono semantics agree with the
trigger family at oscillator rate. There is no consistent way to
collapse two signed streams to one for either purpose: `(L + R) / 2`
cancels antiphase content; `max(|L|, |R|)` is rectified magnitude (not a
signed value, contradicting the mono semantics — a stereo gate keyed off
`max(|L|, |R|)` would behave as "envelope above threshold", a different
operation from the mono gate's signed schmitt at the same threshold);
and per-channel detection contradicts a single output. A patch needing
a stereo-bus gate or trigger derives it from per-channel detector
instances fed by `StereoSplitter`, or from a dedicated stereo
magnitude-summariser (peak / RMS over both channels) whose mono output
feeds an `AudioToGate` when envelope semantics are wanted. The
`StereoCompressor` / `StereoGate` dynamics modules can use `max(|L|, |R|)`
linking precisely because their detector is *already* a rectified
magnitude (envelope-following), so the linking matches their mono
counterpart's semantics; the trigger/gate detectors compare signed
samples, so the same trick would change their semantics.

Edge detector design:

- **What is detected.** A threshold-crossing of the instantaneous
  signed sample value, *not* envelope / magnitude / onset. Use cases:
  clock recovery from oscillator-rate audio, sub-octave division,
  feeding hard-sync from an arbitrary audio source (the ADR 0047
  motivating case). Drum/transient onset detection is a different
  problem (envelope follower → threshold) and is out of scope here.
- **Fire condition.** `armed && prev <= threshold && curr > threshold`
  (rising direction; falling and bidirectional follow symmetrically).
- **Sub-sample position.** `frac = (threshold - prev) / (curr - prev)`,
  expressed as a fractional position in ADR 0047's event encoding.
  The fire threshold is `threshold`, never `threshold - hysteresis`.
- **Hysteresis.** Two-state machine: `armed` ↔ `disarmed`. Re-arm
  requires `signal < threshold - hysteresis` for rising direction.
  Hysteresis controls *eligibility*, never event location. A code
  comment at the interp line and a doc-comment note state this so the
  asymmetry is not "fixed" by a future reader.
- **Cooldown.** Optional debounce param. Suppresses re-fires for N
  samples (or ms) after a fire, even if the signal re-arms.

`AudioToGate` reuses the same hysteresis state machine but holds the
gate high while `armed && signal > threshold`; the gate falls when
`signal < threshold - hysteresis`. No sub-sample reporting — gates
are sample-accurate by ADR 0030.

#### Stereo utility group

| Module        | Ports                                                                        | Notes                                                                                                                                                        |
| ------------- | ---------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `Pan`         | `in: mono`, `pan: mono` (CV), `out: stereo`                                  | Equal-power law (sin/cos, −3 dB centre). `pan: -1..1`                                                                                                        |
| `Balance`     | `in: stereo`, `balance: mono` (CV), `out: stereo`                            | Linear −6 dB, matches mixer pan law                                                                                                                          |
| `StereoWidth` | `in: stereo`, `width: mono` (CV), `out: stereo`                              | Internal M-S: scale S by `width`, leave M unchanged. `width: 0..2` (0 = mono sum, 1 = unchanged, 2 = double-width)                                           |
| `MidSide`     | `stereo_in: stereo`, `ms_out: stereo`, `ms_in: stereo`, `stereo_out: stereo` | Bidirectional. Encodes LR → (M=L+R)/2, S=(L−R)/2 on one path; decodes inverse on the other. The two paths are independent — connect either or both as needed |
| `MonoBass`    | `in: stereo`, `cutoff: mono` (CV), `out: stereo`                             | Linkwitz-Riley 4th order crossover (−24 dB/oct). Below cutoff: `(L+R)/2` fed to both channels. Above cutoff: unchanged stereo. Default cutoff 120 Hz         |

`MidSide` ports are all `Stereo` cables — the M-S form is a stereo
cable carrying `(M, S)` rather than `(L, R)`. The module is
purely an arithmetic relabel; no descriptor metadata distinguishes M-S
stereo from L-R stereo. Patch authors must keep them straight (the
same constraint that already applies to mid/side workflows in any
DAW).

#### Primitives group

| Module      | Ports                   | Notes                                                                                   |
| ----------- | ----------------------- | --------------------------------------------------------------------------------------- |
| `DcBlocker` | `in: mono`, `out: mono` | Thin wrapper over `patches_dsp::DcBlocker`. No params; fixed cutoff per existing kernel |
| `Comb`      | `in: mono`, `out: mono` | Single module with `mode` enum (`ff` / `fb` / `both`), `delay_ms`, `feedback`, `mix`    |

### Sidechain convention

Sidechain input ports use the name `sidechain`. Single word, matches
the existing convention (`feedback`, `voct`, `gate`) over abbreviation
(`sc`).

**Unconnected fallback: self-key.** When `sidechain` is unconnected,
the detector reads from `in` instead. This is the standard hardware
behaviour: a compressor with nothing plugged into its sidechain
compresses its own input. The "no sidechain wired" case is the
common one and must not require explicit patching.

This is implemented by the module reading both `in` and `sidechain`
each tick and selecting the detector source based on connectivity,
reported via `MonoInput::is_connected()` (or the cable kind's
equivalent). No `Option`-typed cable read on the audio path.

For stereo dynamics, the sidechain port is stereo. A mono source
feeding it is silently broadcast (`L = R = source`) by the existing
ADR 0059 rule.

### Stereo dynamics: true-stereo linking

Stereo compressor and gate use **one** detector. The detector input is:

- **Peak mode:** `max(|L|, |R|)` (the existing `StereoLimiter`
  convention).
- **RMS mode:** `sqrt((L² + R²) / 2)`.

A single gain-reduction value (compressor) or gate state (gate) is
computed and applied identically to L and R. This preserves the
stereo image under transients — independent L/R detection would cause
audible image shift on panned hits. Without this, a stereo "comp" is
just two mono comps in a trenchcoat and earns nothing over patching
two `Compressor` instances by hand.

### Source-tree rationalisation

`patches-modules/src/` is reorganised into semantic groups, each a
directory with `mod.rs` collecting the public surface and per-variant
subfiles. Target layout (only directories named; subfiles follow the
existing `mixer/{mono,stereo,poly,stereo_poly}.rs` pattern):

```text
patches-modules/src/
├── common/             # primitives shared across modules (existing)
├── dynamics/           # compressor, gate, limiter, transient_shaper
├── stereo/             # pan, balance, width, mid_side, mono_bass,
│                       # stereo_split, stereo_sum
├── detectors/          # audio_to_trigger, audio_to_gate,
│                       # trigger_sync_conv
├── filter/             # (existing) + eq when ADR 0077 lands
├── mixer/              # (existing)
├── modulators/         # adsr, lfo, sah, glide, op, quant, tuner,
│                       # ring_mod
├── osc/                # oscillator (mono + poly variants)
├── delay/              # delay, stereo_delay
├── reverb/             # fdn_reverb
├── effects/            # bitcrusher, drive
├── midi/               # midi_arp, midi_cc, midi_delay,
│                       # midi_drumset, midi_source, midi_split,
│                       # midi_to_cv, midi_transpose
├── sequencer/          # master_sequencer, pattern_player,
│                       # tracker_core
├── host/               # host_control, host_transport, audio_in,
│                       # audio_out, ms_ticker, tempo_sync
├── utils/              # sum, vca, tap, mono_to_poly, poly_to_mono,
│                       # quant_util
└── primitives/         # dc_blocker, comb
```

Each group's directory consolidates mono/poly/stereo variants of a
conceptual module into subfiles. Top-level `pub use` re-exports
preserve every existing public path (`patches_modules::Compressor`,
`patches_modules::ResonantLowpass`, etc.) — no breaking changes for
downstream crates.

Migration is one ticket per group, all under one epic. Tickets land
in dependency order (`common` → leaf groups → cross-cutting ones).
Each migration is structural only: no behaviour changes, no public
API changes, no test changes beyond import paths.

### Implementation pattern: DSP/module separation and testing

Every new module in this ADR (and, going forward, in the native
bundle) follows the same structural split:

- **DSP / algorithm kernel** lives outside the module struct. Either:
  - In a `common/` submodule inside the module's group directory
    (e.g. `dynamics/common/comp_detector.rs`,
    `detectors/common/edge.rs`) when the kernel is specific to this
    group, or
  - In `patches-dsp` when the kernel is reusable across contexts (a
    different bundle, a different group). The bar for promoting to
    `patches-dsp` is "second consumer exists or is realistically
    imminent" — premature promotion adds version-cadence cost.
- **Kernel tests** are colocated with the kernel and test it
  independently of any module wiring. Property tests, golden curves,
  invariant checks. No `ModuleHarness`, no descriptor, no cable
  pool. The kernel is plain Rust state + functions.
- **Module surface tests** cover only the wiring behaviour: parameter
  routing, descriptor shape, and *behaviour that varies with
  connectivity or enum settings*. Specifically:
  - Sidechain self-key fallback (connected vs unconnected sidechain
    must produce visibly different detector behaviour).
  - Detector mode (`peak` vs `rms` selecting the right kernel
    branch).
  - `Comb` mode (`ff` / `fb` / `both`).
  - `AudioToTrigger` direction (`rising` / `falling` / `both`).
  - `MidSide` partial wiring (only encode side connected; only decode
    side connected; both connected simultaneously).
  - Stereo dynamics linked-detector behaviour (image preservation
    under asymmetric L/R transients — the test that catches a future
    "fix" reverting to per-channel detection).
- **No module-surface test for things derivable from the kernel.** If
  a parameter passes straight through to the kernel and the kernel
  test covers the behaviour, the module test would re-test the
  kernel through extra plumbing for no additional signal.

This matches the established pattern in `mixer/` (kernel-thin module,
behaviour in mute/solo/pan logic tested at the module level because
those *are* the wiring) and `fdn_reverb/` (kernel + line + matrix in
separate files, processor tests the assembly). New modules apply it
deliberately rather than by accident.

### Things explicitly not in this ADR

- **Parametric / shelving / graphic EQ.** Multiple flavours, multiple
  param surfaces, separate ADR (0077). The `filter/` group has a slot
  for it; nothing in this ADR blocks it.
- **Wavefolder as a separate module.** `Drive` already has a `fold`
  mode using `fast_sine`. Adding a sibling module duplicates the
  shaper without distinct value.
- **Expander.** A downward expander is a generalisation of `Gate`
  (ratio < ∞). YAGNI for v1 — the gate's binary semantics cover the
  common case and a separate `Expander` can ship when a real use case
  appears.
- **Phaser.** Belongs in `patches-vintage`, not native.
- **`AudioIn` host channel expansion.** Sidechain ports are wired
  internally to whatever the patch routes there. Exposing additional
  host audio channels (for true outboard sidechain from the host) is
  E148 / future scope.
- **Multi-band dynamics.** Out of scope for native stdlib.

## Consequences

### Positive

- Two structural holes (dynamics, stereo utility) closed with a
  coherent design rather than ad-hoc per-module choices.
- One sidechain convention (`sidechain` port name, self-key fallback)
  applied across comp/gate at introduction, avoiding a second
  retrofit pass.
- True-stereo dynamics earn their separate-module status; trivial
  L/R-independent versions are not built.
- Sub-sample edge timing is correctly defined under hysteresis at
  design time. The interp/hysteresis trap is documented before any
  code lands.
- Source tree reorganisation removes the long-standing flat-vs-grouped
  inconsistency. Future modules have an obvious home.
- New modules land in groups from the start; the reorg epic backfills
  existing modules.

### Negative

- Source-tree reorganisation is a large structural diff. Mitigated
  by per-group tickets, structural-only changes, and preserved public
  re-exports.
- `MidSide` as a single bidirectional module is unconventional. A
  user expecting `MsEncode` / `MsDecode` separately must read the
  module doc to discover the four-port form. Mitigated by clear
  documentation and the fact that either direction can be used
  standalone (the other half is just unconnected).
- `sidechain` self-key fallback requires a connectivity check on the
  audio path. The check is cheap (`is_connected()` boolean) but is
  one more branch per tick per stereo dynamic.
- Adding ten new module types grows the bundle surface area and the
  CLAP/LSP registry footprint. Cost is small; benefit is greater.

### Migration / follow-up

- Epic E151 (next free): native dynamics + stereo utility + source
  tree reorg. Tickets:
  - Sidechain convention + self-key reads on existing limiter as a
    proof point (or document why limiter does not get one).
  - `Compressor` / `StereoCompressor`.
  - `Gate` / `StereoGate`.
  - `AudioToTrigger` / `PolyAudioToTrigger` (no stereo variant — see
    "no stereo variants in either family" above).
  - `AudioToGate` / `PolyAudioToGate` (no stereo variant — see
    "no stereo variants in either family" above; a stereo peak/RMS
    summariser module is a follow-up if the use case appears).
  - `Pan`, `Balance`, `StereoWidth`, `MidSide`, `MonoBass`.
  - `DcBlocker`, `Comb`.
  - Source-tree reorg: one ticket per target group directory.
  - Manual / module-reference docs update.
- ADR 0077 (separate): EQ flavours and parametric vs shelving vs
  graphic decision.
