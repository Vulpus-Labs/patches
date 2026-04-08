# Polyphonic synth

`examples/poly_synth.patches` — a straightforward polyphonic subtractive
synthesiser: oscillator → filter → VCA, driven by MIDI.

## Running it

```bash
cargo run -p patches-player -- examples/poly_synth.patches
```

## What to tweak live

- `env` parameters (attack, decay, sustain, release) shape the amplitude
- Filter cutoff and resonance
- Oscillator waveform (try `.saw` instead of `.sine`)
