---
id: "0609"
title: Define new C ABI function typedefs + HostEnv vtable in patches-ffi-common
priority: high
created: 2026-04-21
---

## Summary

Add the ADR 0045 §6 C-ABI function typedefs and `HostEnv` vtable
to `patches-ffi-common`. Pure definitions — no callers yet.

## Acceptance criteria

- [ ] `extern "C"` fn typedefs for `update_validated_parameters`,
      `set_ports`, `process` per ADR 0045 §6.
- [ ] `HostEnv` struct: `float_buffer_release: extern "C" fn(u64)`,
      `song_data_release: extern "C" fn(u64)`.
- [ ] `Handle = *mut c_void` newtype or type alias; `#[repr(transparent)]`
      where appropriate.
- [ ] Unit test asserts `HostEnv` is `#[repr(C)]` and field layout
      is stable (use `memoffset` or manual offset_of).
- [ ] `cargo clippy -p patches-ffi-common` clean.

## Notes

Epic E103. References ADR 0045 §6 and §5 (PortFrame).
