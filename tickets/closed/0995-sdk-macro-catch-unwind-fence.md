---
id: "0995"
title: catch_unwind fence in plugin SDK export macros
priority: high
created: 2026-06-11
---

## Summary

The `export_plugin!` / `export_modules!` macros
(`patches-ffi-common/src/sdk.rs`, generated entry points
`__patches_process`, `__patches_periodic_update`,
`__patches_update_validated_parameters`, `__patches_set_ports`, ...)
invoke user `Module` code directly inside `extern "C"` functions with no
`catch_unwind`. Since Rust 1.81 an unwind out of `extern "C"` is a
guaranteed process abort — a panicking third-party module kills the DAW
before the host-side ADR 0051 fence in `PatchProcessor::tick` can halt
cleanly.

## Acceptance criteria

- [x] Every generated `extern "C"` body wraps user code in
      `std::panic::catch_unwind(AssertUnwindSafe(...))` — centralised in
      `sdk::fence` + per-entry `*_dispatch` helpers, mirroring the existing
      `prepare_dispatch`.
- [x] On `Err`: audio-thread entries return `FFI_ENTRY_PANIC`; the host
      loader (`DylibModule::ffi_panic`) re-raises into the tick / callback
      fence, feeding the existing halt machinery (clean halt + diagnostic).
- [x] Applies to `export_plugin!`, `export_modules!`, and
      `export_plugin_with_hash_override!`; hand-written `release-on-update`
      fixture updated to the `-> i32` signatures.
- [x] Integration test `ffi_panic_halt::panic_in_dylib_process_halts_cleanly`:
      a panicking dylib (`test-panic-plugin`) processes one tick; host halts
      cleanly (slot 0, `PanicOnProcess`), process survives, halt sticky.
- [x] ADR 0051 amended (E163 amendment, point 2) to name the SDK macros as
      the plugin-side fence.

## Resolution

ABI v13 changed `process` / `set_ports` / `update_validated_parameters` to
return `i32` (`FFI_ENTRY_OK` / `FFI_ENTRY_PANIC`); `periodic_update` keeps
0/1 and adds the panic sentinel. Plugin-side fence lives in
`patches-ffi-common/src/sdk.rs`; host re-raise in
`patches-ffi/src/loader.rs`. Closed under **E163**.

## Notes

Part of **E163**. Host-side `catch_unwind` at
`patches-engine/src/processor.rs:628` cannot help here: the abort happens
in the plugin's own `extern "C"` frame before control returns.
Panic-in-FFI sentinel design should reuse the halt-state plumbing from
`patches-engine/src/halt.rs` rather than inventing a parallel channel.
