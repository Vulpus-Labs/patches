---
id: "E106"
title: ADR 0045 spike 7 phase D — port gain plugin; delete stale bundles
created: 2026-04-21
depends_on: ["E104", "E105"]
tickets: ["0619", "0620"]
---

## Goal

Prove the new ABI end-to-end with one live dylib. After this epic:

- `test-plugins/gain` rewritten on `export_plugin!`. Dylib loads
  through the new loader, descriptor hash matches, audio output
  bit-identical to pre-migration capture at the same parameter
  values.
- `test-plugins/conv-reverb`, `test-plugins/drums-bundle`,
  `test-plugins/gain-wasm`, `test-plugins/old-abi` deleted
  outright. FFI subsystem has no external users; resurrection
  cost is low, maintenance cost against a changing ABI is not.
  `patches-vintage` migration (spike 8) will be done on the new
  ABI from scratch.

## Tickets

| ID   | Title                                                       | Priority | Depends on |
| ---- | ----------------------------------------------------------- | -------- | ---------- |
| 0619 | Rewrite test-plugins/gain on export_plugin! + new ABI       | high     | E104, E105 |
| 0620 | Delete conv-reverb, drums-bundle, gain-wasm, old-abi        | medium   | 0619       |

## Definition of done

- `cargo build -p gain --release` produces a `.dylib` that loads
  through `patches-ffi` loader without error.
- End-to-end: load gain bundle, process 1 s of signal through
  `patches-player`, output matches reference WAV bit-identically.
- Workspace `Cargo.toml` members list no longer references the
  four deleted plugin crates.
- `cargo build --workspace` clean.
