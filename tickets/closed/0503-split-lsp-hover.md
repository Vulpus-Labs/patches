---
id: "0503"
title: Split patches-lsp hover.rs by target kind
priority: medium
created: 2026-04-16
---

## Summary

`patches-lsp/src/hover.rs` is 750 lines of hover handlers,
dispatching on cursor-context kind and building hover strings for
modules, ports, parameters, and template references.

## Acceptance criteria

- [ ] Convert to `hover/mod.rs` with submodules:
      `module.rs` (module type / instance hover),
      `port.rs` (port hover including poly layout info),
      `param.rs` (parameter hover),
      `template.rs` (template call / template port hover).
- [ ] Top-level `hover` entry point and any shared helpers stay in
      `mod.rs`.
- [ ] Each submodule under ~300 lines; `mod.rs` under ~200.
- [ ] `cargo build -p patches-lsp`, `cargo test -p patches-lsp`,
      `cargo clippy` clean.

## Notes

E086. No behaviour change.
