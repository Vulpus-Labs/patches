---
id: "0626"
title: Delete dead FFI code — JSON audio-path + FfiPort structs
priority: medium
created: 2026-04-21
---

## Summary

Post-Phase D, several types and functions have no callers:
JSON param serialisers, `FfiInputPort` / `FfiOutputPort` if the
`PortFrame` rewrite obsoleted them, any stale loader helpers.
Delete them; do not leave behind `#[allow(dead_code)]`.

## Acceptance criteria

- [ ] `cargo clippy -p patches-ffi -- -D dead_code` clean.
- [ ] `cargo clippy -p patches-ffi-common -- -D dead_code`
      clean.
- [ ] No JSON functions reachable from the three audio entry
      points (verifiable by grep).

## Notes

Epic E108.
