---
id: "0268"
title: "Test plugin: ConvolutionReverb cdylib"
priority: high
created: 2026-04-07
---

## Summary

Compile the ConvolutionReverb as an external cdylib plugin and write
integration tests that exercise threads, file I/O, PeriodicUpdate, and correct
cleanup. This is the validation target from ADR 0025 — if this works, the
plugin system is production-ready.

## Acceptance criteria

- [ ] A `test-plugins/conv-reverb/` directory with a cdylib crate that wraps `ConvolutionReverb` from `patches-modules` via `export_module!`
- [ ] Integration tests:
  - **Basic lifecycle**: load plugin, build with `ir=room` (synthetic IR, no file I/O), process N samples, verify non-silent output, drop cleanly
  - **File I/O**: build with `ir=file`, `path=<test fixture wav>`, verify output from convolution with the loaded IR
  - **File error propagation**: build with `ir=file`, `path=/nonexistent` — `update_parameters` returns error across FFI boundary
  - **Parameter update restarts thread**: build with `ir=room`, update to `ir=hall` — verify old processing thread is joined and new one is spawned (no thread leak)
  - **PeriodicUpdate**: connect a CV input to the mix port, verify mix parameter is modulated via `periodic_update` across the FFI boundary
  - **Drop joins threads**: build with processing thread running, drop `DylibModule`, verify thread is joined before drop returns (thread count check or timing assertion)
  - **Cleanup-thread drop**: send `DylibModule` to another thread, drop it there (simulates engine cleanup_tx path), verify clean shutdown
  - **Multiple instances**: load two instances from same plugin, drop one, verify the other continues to work (Arc<Library> keeps lib alive)
- [ ] `cargo clippy` clean

## Notes

- This crate depends on `patches-modules` (for `ConvolutionReverb`),
  `patches-core`, `patches-ffi`, and `patches-dsp`. It re-exports the module
  through the FFI macro rather than reimplementing it.
- Thread-count assertions can use `/proc/self/task` on Linux or
  `std::thread::current().id()` probes. Alternatively, use a timeout: if drop
  returns promptly, threads were joined; if it hangs, they were not.
- A small test WAV fixture (a few hundred samples of an impulse) should be
  committed to the test-plugins directory for the file I/O test.

Epic: E052
ADR: 0025
Depends: 0267
