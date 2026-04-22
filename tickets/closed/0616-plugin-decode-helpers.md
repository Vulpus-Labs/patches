---
id: "0616"
title: Plugin-side decode helpers — ParamView / PortView from bytes
priority: high
created: 2026-04-21
---

## Summary

Plugins need to reconstruct `ParamView` / `PortView` from the
bytes the host hands them. The helpers live in
`patches-ffi-common` so both host tests and plugin crates share
one implementation.

## Acceptance criteria

- [ ] `fn decode_param_frame<'a>(bytes: &'a [u8], layout: &'a ParamLayout, index: &'a ParamViewIndex) -> ParamView<'a>`.
- [ ] `fn decode_port_frame<'a>(bytes: &'a [u8], layout: &'a PortLayout) -> PortView<'a>`.
- [ ] Length mismatch: debug-panic, release-return-Err (matching
      the Spike 5 pack guard style).
- [ ] Unit test: pack then decode round-trips for both.

## Notes

Epic E105.
