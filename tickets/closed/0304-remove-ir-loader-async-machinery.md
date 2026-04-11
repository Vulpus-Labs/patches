---
id: "0304"
title: "patches-modules: remove IrLoader async machinery"
priority: medium
created: 2026-04-11
---

## Summary

Remove the `IrLoader` background thread, `IrLoadRequest`, `ProcessorReady`,
and `ProcessorTeardown` types from ConvolutionReverb and
StereoConvReverb, now that file loading and FFT processing are handled by
the planner via `FileProcessor`.

## Acceptance criteria

- [ ] `IrLoader` struct and `ir_loader_main` function removed
- [ ] `IrLoadRequest`, `ProcessorReady`, `ProcessorTeardown` types removed
- [ ] `pending_request` field removed from `ConvolutionReverb` and `StereoConvReverb`
- [ ] `periodic_update` no longer polls for async IR load results (still needed for mix CV and other periodic work)
- [ ] `resolve_ir` and `resolve_stereo_ir` functions are either removed or relocated (synthetic IR generation may still be needed for built-in variants)
- [ ] The per-module background thread for overlap-add processing is retained — only the IR *loading* thread is removed
- [ ] No regression in audio output for synthetic IR variants (`room`, `hall`, `plate`)
- [ ] `cargo test -p patches-modules` passes
- [ ] `cargo clippy -p patches-modules` clean

## Notes

This is a cleanup ticket that follows 0303. The IrLoader machinery is ~200
lines of lifecycle management code (spawn, teardown channels, ring buffers,
shutdown flag, thread join). Removing it significantly simplifies the
module.

The overlap-add processor thread (which convolves audio blocks against the
IR) is a separate concern and must be retained — it is part of the real-time
processing pipeline, not the file loading pipeline.

Epic: E056
ADR: 0028
Depends: 0303
