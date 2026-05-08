---
id: "0838"
title: Type the FFI `prepare` boundary (`PrepareResult`, `NonNull` handle)
priority: medium
created: 2026-05-08
epic: E139
---

## Summary

`PluginLoader::prepare_instance` (in [patches-ffi/src/loader.rs:199-228](../../patches-ffi/src/loader.rs#L199))
returns success via three independent signals:

1. an `i32` status code (`PREPARE_OK` vs others),
2. a possibly-null `*mut c_void` handle,
3. a possibly-null `FfiBytes` error message.

The caller checks all three by hand. The unsafe block extends past the
plugin call into the result-decoding logic, mixing wire-protocol concerns
with Rust-level error propagation.

A single `unsafe fn` shim that consumes the three out-params and returns
`Result<NonNull<c_void>, PrepareError>` collapses the unsafe surface,
makes "got a handle ⇒ status was OK" structurally true, and removes the
post-condition null check that currently lives in safe code.

## Sites

- [patches-ffi/src/loader.rs:199](../../patches-ffi/src/loader.rs#L199)
  — `*mut c_void = null_mut()` field.
- [patches-ffi/src/loader.rs:204-228](../../patches-ffi/src/loader.rs#L204)
  — manual decode of (status, handle, error_bytes) tuple.

## Proposed shape

```rust
enum PrepareResult {
    Ok(NonNull<c_void>),
    Err(String), // already-decoded UTF-8 lossy
}

// Safety: vtable.prepare must conform to FfiPlugin ABI v1.
unsafe fn call_prepare(
    vtable: &FfiVTable,
    desc_json: &[u8],
    env: FfiAudioEnvironment,
    instance_id: InstanceId,
    structural_blob: &[u8],
) -> PrepareResult { ... }
```

Caller becomes:

```rust
let handle = match unsafe { call_prepare(...) } {
    PrepareResult::Ok(h) => h,
    PrepareResult::Err(message) => {
        return Err(BuildError::Custom { module: ..., message, origin: None });
    }
};
```

## Acceptance criteria

- [ ] `unsafe` block in `prepare_instance` covers only the FFI call, not
      the result decoding
- [ ] Plugin handle stored as `NonNull<c_void>` post-prepare (or
      `Option<NonNull<c_void>>` if a delayed-init phase exists)
- [ ] `PREPARE_OK` magic constant either replaced by an exhaustive
      status enum at the FFI boundary, or constrained to the shim and
      not visible to callers
- [ ] Existing FFI integration tests pass (`patches-ffi/tests/`)
- [ ] `just commit -p patches-ffi` clean

## Notes

Pairs naturally with 0839 (`ValidatedParamFrame`); land 0838 first so
0839 can build on the typed handle. Out of scope: changing the FFI ABI
itself — this ticket is purely Rust-side decoding.

Cross-reference: project memory `project_ffi_design.md` notes per-sample
process is raw-pointer cheap; this ticket only touches the control-rate
prepare path, so the marshalling cost is already amortised.
