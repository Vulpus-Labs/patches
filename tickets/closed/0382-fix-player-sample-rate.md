---
id: "0382"
title: Fix player hardcoded sample rate
priority: medium
created: 2026-04-13
---

## Summary

In `patches-player/src/main.rs` line 37, `load_patch` hardcodes
`sample_rate: 44_100.0` in the `AudioEnvironment` regardless of the actual
audio device sample rate. The interpreter builds module graphs with this
environment, but the engine may run at 48000 Hz or another rate. If any
interpreter or module logic is sample-rate-dependent at build time, this
produces incorrect results.

## Acceptance criteria

- [ ] `load_patch` receives the actual device sample rate (or a reasonable default that is corrected after device init)
- [ ] On hot-reload, the correct sample rate is used
- [ ] Two separate `default_registry()` calls (lines 72–73) consolidated into one shared reference if possible

## Notes

The sample rate is only known after `PatchEngine::start()`. Options:
1. Build with a placeholder, then rebuild immediately after start with the real rate.
2. Query the device sample rate before starting the engine.
3. Accept 44100 as default and document the limitation.

Option 2 is cleanest — `enumerate_devices` or a similar query could provide the
rate before engine construction.
