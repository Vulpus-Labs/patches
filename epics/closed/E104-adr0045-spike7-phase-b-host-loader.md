---
id: "E104"
title: ADR 0045 spike 7 phase B — host loader rewrite
created: 2026-04-21
depends_on: ["E103"]
tickets: ["0612", "0613", "0614", "0615"]
---

## Goal

Flip `patches-ffi/src/loader.rs` to the new ABI. JSON gone from
audio path. Descriptor-hash checked at load. `PortFrame`
dispatched instead of per-call `Vec` alloc. ABI version bumped;
old plugins refused cleanly.

After this epic `patches-ffi` compiles but plugins are broken
until E105/E106 deliver the SDK + ported gain. FFI has no
external users; the broken-window gap between epics is
acceptable and kept short by running E105 in parallel with E104.

## Tickets

| ID   | Title                                                        | Priority | Depends on |
| ---- | ------------------------------------------------------------ | -------- | ---------- |
| 0612 | Loader: ParamFrame dispatch via new ABI; drop JSON           | high     | E103       |
| 0613 | Loader: PortFrame dispatch via plan channel; drop Vec allocs | high     | E103, 0612 |
| 0614 | Load-time descriptor_hash check; refuse mismatch             | high     | E103       |
| 0615 | Bump manifest ABI version; delete JSON audio-path code       | high     | 0612, 0613 |

## Definition of done

- `patches-ffi/src/loader.rs`: no `json::` on audio entry
  points.
- `DylibModuleBuilder` construction fails on hash mismatch
  before `create` is called.
- Manifest ABI version incremented; old plugins rejected at
  scan.
- `cargo build -p patches-ffi` clean (ignore downstream plugin
  build failures — gain gets rewritten in E106).
