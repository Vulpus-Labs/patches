---
id: "E105"
title: ADR 0045 spike 7 phase C — plugin-side SDK
created: 2026-04-21
depends_on: ["E103"]
tickets: ["0616", "0617", "0618"]
---

## Goal

Give plugin authors an `export_plugin!` macro that hides the
`extern "C"` glue. After this epic a plugin crate is roughly:

```rust
struct MyModule { /* state */ }
impl Module for MyModule { /* existing trait */ }
patches_ffi_common::export_plugin!(MyModule, descriptor_fn);
```

No hand-written `#[no_mangle]`. No JSON. No unsafe in user code.

After this epic `patches-ffi-common` exports:

- `decode_param_frame(bytes, layout, index) -> ParamView<'_>` —
  zero-alloc view constructor for the plugin side.
- `decode_port_frame(bytes, layout) -> PortView<'_>` — same for
  ports.
- `export_plugin!` macro — emits all ABI symbols
  (`descriptor_hash`, `create`, `destroy`, `prepare`, `describe`,
  `update_validated_parameters`, `set_ports`, `process`) that
  forward into a user-supplied `Module` impl and descriptor fn.

This runs in parallel with Phase B (E104); they meet in Phase D.

## Tickets

| ID   | Title                                                          | Priority | Depends on |
| ---- | -------------------------------------------------------------- | -------- | ---------- |
| 0616 | Plugin-side decode helpers: ParamView / PortView from bytes    | high     | E103       |
| 0617 | export_plugin! macro: emit all #[no_mangle] ABI symbols        | high     | 0616       |
| 0618 | SDK smoke test: minimal in-crate Module round-trip via macro   | high     | 0617       |

## Definition of done

- `cargo expand` on a trivial `export_plugin!` invocation
  produces the expected 8 extern symbols.
- `patches-ffi-common` smoke test: mock `Module` wired through
  the macro decodes a `ParamFrame` and matches expected values
  (no actual cdylib involved yet — the macro body is exercised
  inline).
- `cargo clippy -p patches-ffi-common` clean with no `unsafe`
  warnings suppressed outside documented ABI boundary points.
