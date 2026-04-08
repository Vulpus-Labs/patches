# FM synthesiser

`examples/fm_synth.patches` — a polyphonic FM synthesiser driven by MIDI.

## Architecture

Two polyphonic oscillators: a *carrier* (`osc_a`) and a *modulator* (`osc_b`).
The modulator's output, shaped by its own ADSR envelope and VCA, is fed into
the carrier's phase modulation input. The carrier's output passes through a VCA
(shaped by the volume envelope), collapses to mono, then goes through a filter
to the output.

```
kbd ──► vol_adsr ──► vca ──► to_mono ──► filter ──► out
kbd ──► osc_a ──────────────────────────────────────►
kbd ──► mod_adsr ──► mod_vca ─[0.3]──► osc_a.phase_mod
kbd ──► osc_b ──────────────────────────────────────►
```

## Running it

```bash
cargo run -p patches-player -- examples/fm_synth.patches
```

Connect a MIDI keyboard before starting. `PolyMidiIn` uses the first available
MIDI port.

## What to tweak live

- `mod_adsr` decay and sustain control the brightness evolution per note
- `osc_b.frequency` sets the modulator ratio (try integer multiples of `osc_a`)
- The `-[0.3]->` scale on the FM cable controls modulation depth
- `filter.cutoff` shapes the overall timbre
