---
id: "E103"
title: ADR 0045 spike 7 phase A — new C ABI surface in patches-ffi-common
created: 2026-04-21
depends_on: ["ADR 0045 spike 5 (E101)", "ADR 0045 spike 6 (E108-equiv, 0608)"]
tickets: ["0609", "0610", "0611"]
---

## Goal

Land the shared type surface both host and plugin compile against.
Pure definitions — no host behaviour, no plugin glue. After this
epic `patches-ffi-common` exports:

- C-ABI function typedefs: `update_validated_parameters`,
  `set_ports`, `process`, plus existing `create`/`destroy`/
  `prepare`/`describe`.
- `HostEnv` vtable (`float_buffer_release`, `song_data_release`).
- `descriptor_hash` — deterministic `u64` from `ModuleDescriptor`,
  identical across host and plugin compile units.
- `PortFrame` layout (C-repr header + trailing port structs).

JSON codec stays in `patches-ffi-common::json` strictly for
descriptor/manifest exchange at load. Nothing else uses it yet.

Implements ADR 0045 spike 7 setup. Audio-thread allocation is
impossible by construction — there is no behaviour to allocate in.

## Tickets

| ID   | Title                                                | Priority | Depends on |
| ---- | ---------------------------------------------------- | -------- | ---------- |
| 0609 | Define new C ABI function typedefs + HostEnv vtable  | high     | —          |
| 0610 | descriptor_hash: stable u64 over ModuleDescriptor    | high     | —          |
| 0611 | PortFrame wire format + encode/decode helpers        | high     | 0609       |

## Definition of done

- `patches-ffi-common` compiles clean with new types.
- `cargo test -p patches-ffi-common` covers: hash determinism
  (same descriptor ⇒ same hash, stable across process runs),
  PortFrame round-trip.
- No host / plugin / loader changes yet. Everything below the
  ABI still runs the old path.
