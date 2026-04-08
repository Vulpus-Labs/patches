# Layered poly synth

`examples/poly_synth_layered.patches` — demonstrates templates by defining a
reusable `voice` template and instantiating multiple layers.

## Running it

```bash
cargo run -p patches-player -- examples/poly_synth_layered.patches
```

## How templates are used

A `voice` template encapsulates the oscillator → filter → VCA chain. Multiple
instances with different parameters (detuning, filter cutoff) are instantiated
in the patch and blended at the output. The expander inlines each instance
before the graph is built, so there is no template overhead at runtime.
