# patches-dsp

Pure-DSP building blocks for the Patches modular-audio framework:
biquad and state-variable filters, ladder filters, halfband
interpolation/decimation, sinc resampling, delay buffers with Thiran
all-pass interpolation, peak/RMS windows, ADSR core, noise PRNG, fast
math approximations, and oscillator phase accumulators with PolyBLEP.

No audio backend, no module-protocol concerns: kernels only. Lives
behind [`patches-core`][core] in the workspace but has no dependency
on it, so non-Patches projects can use these primitives directly.

License: MIT.

[core]: https://crates.io/crates/patches-core
