# Noise generators

> **Source of truth:** the doc comments on each module struct in
> `patches-modules/src/` define the canonical port names, parameter
> ranges, and behaviour. This page is kept in sync with those comments.

## `Noise` — Mono noise generator

Four coloured noise outputs from a single module. Only connected outputs are
computed, so unused colours have no CPU cost.

The four outputs span a range of spectral slopes. White noise is the source for
all other colours: pink is derived by filtering white, brown by integrating
white, and red by integrating brown.

**Outputs**

| Port | Spectrum | Description |
| --- | --- | --- |
| `white` | flat | Uncorrelated samples, equal energy per Hz band |
| `pink` | −3 dB/oct (1/f) | Equal energy per octave; useful for natural-sounding randomness and modulation |
| `brown` | −6 dB/oct (1/f²) | Random-walk integration of white; deep, slow-moving character |
| `red` | −9 dB/oct (1/f³) | Integration of brown; very slow drift, sub-bass emphasis |

All outputs are in the range `[−1, 1]`. Each instance has its own PRNG state
seeded from the module's instance ID, so two `Noise` modules in the same patch
produce independent signals.

**Example — white noise as a sound source**

```text
Noise noise
AudioOut out

noise.white -> out.in_left
noise.white -> out.in_right
```

**Example — pink noise for modulation**

```text
Noise lfo_noise
Vca filter_env { frequency: 440Hz }

lfo_noise.pink -> filter_env.cv
```

---

## `PolyNoise` — Polyphonic noise generator

Identical to `Noise` but with poly outputs. Each voice has its own independent
PRNG and filter state, so voices are uncorrelated with each other and with any
`Noise` instance.

**Outputs**

| Port | Spectrum | Description |
| --- | --- | --- |
| `white` | flat | Per-voice white noise (poly) |
| `pink` | −3 dB/oct (1/f) | Per-voice pink noise (poly) |
| `brown` | −6 dB/oct (1/f²) | Per-voice brown noise (poly) |
| `red` | −9 dB/oct (1/f³) | Per-voice red noise (poly) |

`PolyNoise` is useful for adding independent per-voice variation to polyphonic
patches — for example, routing the `white` output through a `PolyVca` to add
noise to each voice's amplitude independently.

**Example — per-voice breath noise**

```text
PolyNoise src
PolyVca noise_vca
PolyAdsr env { attack: 5ms, decay: 100ms, sustain: 0.2, release: 200ms }
PolyMidiIn midi

midi.voct   -> env.gate  (unused — routing gate)
midi.gate   -> env.gate
env.out     -> noise_vca.cv
src.white   -> noise_vca.signal
```
