# patches-sdk

Module-author entry point for the Patches modular-audio framework.

```toml
[dependencies]
patches-sdk = "0.7"
```

`patches-sdk` re-exports the `Module` trait, descriptors, ports,
cables, parameter machinery, and the `export_modules!` macro from
[`patches-core`][core] and [`patches-ffi-common`][ffi]. Anything
reachable from this crate's public API is the supported surface for
external modules; if you need something not here, file an issue.

`patches-dsp` (FFTs, filters, oscillators, envelopes) is
intentionally **not** re-exported — module authors who want DSP
kernels add a git dependency on it from the main repo.

License: MIT.

[core]: https://crates.io/crates/patches-core
[ffi]: https://crates.io/crates/patches-ffi-common
