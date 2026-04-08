---
id: "0292"
title: "Test plugin: gain-wasm"
priority: high
created: 2026-04-08
---

## Summary

Create a minimal Gain module compiled to WASM, mirroring `test-plugins/gain/`
but using `export_wasm_module!` from `patches-wasm-sdk`. This serves as the
validation target for the host-side WASM loader.

## Acceptance criteria

- [ ] `test-plugins/gain-wasm/` crate exists in the workspace
- [ ] `Cargo.toml` specifies `crate-type = ["cdylib"]`, depends on `patches-core` and `patches-wasm-sdk`
- [ ] `lib.rs` implements the `Module` trait for a `Gain` struct (single mono input, single mono output, float `gain` parameter with range 0.0–2.0, default 1.0)
- [ ] Uses `export_wasm_module!(Gain)` to generate WASM exports
- [ ] `cargo build --target wasm32-unknown-unknown -p test-gain-wasm-plugin` produces a `.wasm` file
- [ ] The `.wasm` file contains all expected exports (`patches_describe`, `patches_prepare`, `patches_process`, `patches_set_ports`, `patches_update_validated_parameters`, `patches_update_parameters`, `patches_alloc`, `patches_free`)

## Notes

- The `Module` implementation should be as close to `test-plugins/gain/src/lib.rs`
  as possible. Ideally the same code compiles to both native and WASM — only
  the export macro differs.
- Ensure `rustup target add wasm32-unknown-unknown` is documented as a
  prerequisite.
- The `.wasm` output will be at
  `target/wasm32-unknown-unknown/debug/test_gain_wasm_plugin.wasm`.

Epic: E055
ADR: 0027
Depends: 0291
