---
id: "0796"
title: Update test-plugins to module_template ABI
priority: high
created: 2026-05-02
epic: "E132"
adrs: ["0066"]
depends_on: ["0795"]
---

## Summary

Update `test-plugins/{gain, conv-reverb, structural-string, all-tags}`
and the `export_plugin!` macro in `patches-ffi-common/src/sdk.rs` to
emit a static template blob via the new ABI 8 `module_template`
vtable entry. Remove the old `__patches_describe` codegen.

## Acceptance criteria

- [ ] `export_plugin!` generates `__patches_module_template` symbol.
- [ ] All test plugins build and load successfully.
- [ ] FFI loader integration tests round-trip plugin → template →
      descriptor for each test plugin.
- [ ] Doc comment on `export_plugin!` updated to show new usage.

## Notes

- Exemplifies the SDK story for third-party plugin authors.
- Ensure error path (malformed blob, version mismatch) has a clear
  diagnostic.
