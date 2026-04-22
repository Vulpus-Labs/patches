---
id: "E108"
title: ADR 0045 spike 7 phase F — FFI cleanup + grep gates
created: 2026-04-21
depends_on: ["E107"]
tickets: ["0626", "0627"]
---

## Goal

Final sweep. Remove dead code surfaced by the migration and lock
in grep gates that will keep the new ABI honest.

After this epic:

- `ParameterMap` JSON codec entries reachable only from
  load-time descriptor / manifest paths. No runtime-update
  references.
- `FfiInputPort` / `FfiOutputPort` structs and their `Vec`
  allocation sites deleted if the `PortFrame` rewrite obsoleted
  them; otherwise folded into `PortFrame`'s trailing array.
- Workspace grep gates (CI script) asserting:
  - `json::` not reachable from `patches-ffi::loader` audio
    entry points.
  - `Vec::` / `Box::` / `String::` not constructed inside
    `update_validated_parameters`, `set_ports`, `process` in
    `patches-ffi`.

## Tickets

| ID   | Title                                                             | Priority | Depends on |
| ---- | ----------------------------------------------------------------- | -------- | ---------- |
| 0626 | Delete dead FFI code: JSON audio-path + FfiPort structs           | medium   | E107       |
| 0627 | CI grep gate: no JSON / no alloc on FFI audio path                | medium   | 0626       |

## Definition of done

- `cargo clippy --workspace` clean, no `#[allow(dead_code)]`
  added for FFI types.
- CI grep gate script runs in `cargo test` or as a separate CI
  step; failing the gate fails the build.
- `cargo test --workspace` green.
