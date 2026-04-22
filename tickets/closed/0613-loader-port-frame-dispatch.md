---
id: "0613"
title: Loader — PortFrame dispatch via plan channel; drop Vec allocs
priority: high
created: 2026-04-21
---

## Summary

`set_ports` currently allocates two `Vec<FfiInputPort>` /
`Vec<FfiOutputPort>` per call. Replace with `PortFrame` built on
the control thread as part of the plan, passed as bytes across
the ABI on adopt.

## Acceptance criteria

- [ ] `ExecutionPlan` per-instance entry owns a `PortFrame`
      (built at plan-build time).
- [ ] `DylibModule::set_ports` dispatches the frame's bytes via
      the new extern `set_ports` fn.
- [ ] Zero allocation on the audio path — verifiable under
      allocator trap in E107.
- [ ] `FfiInputPort` / `FfiOutputPort` structs either removed or
      relegated to a single place inside `PortFrame`'s payload
      typing.

## Notes

Epic E104. Depends on 0612 for the extern-fn typedef wiring and
on 0611 for `PortFrame`.
