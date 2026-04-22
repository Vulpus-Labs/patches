---
id: "E107"
title: ADR 0045 spike 7 phase E — integration test suite
created: 2026-04-21
depends_on: ["E106"]
tickets: ["0621", "0622", "0623", "0624", "0625"]
---

## Goal

Lock in the ABI contract with tests that will catch future
regressions. All live in `patches-integration-tests`; none require
hardware audio.

After this epic the following are green in CI:

1. **Round-trip:** host encodes `ParamFrame`, plugin decodes via
   real `extern "C"` call, every scalar tag + buffer slot
   matches input.
2. **Hash mismatch refuses load:** build a plugin whose
   descriptor drifts from the host's expectation; loader rejects
   it with a descriptive error; no plugin init runs.
3. **Allocator trap:** 10 000 `process` cycles through FFI gain
   under `audio-thread-allocator-trap` — silent.
4. **Double-release audit:** debug build traps when a plugin
   calls `float_buffer_release(id)` twice for the same id.
5. **Leak check:** after full engine shutdown the `ArcTable`
   drains to zero live entries; all `Arc<[f32]>` drop on the
   control thread / cleanup worker, never on audio thread.

## Tickets

| ID   | Title                                                          | Priority | Depends on |
| ---- | -------------------------------------------------------------- | -------- | ---------- |
| 0621 | FFI round-trip test: encode → extern C → decode parity         | high     | E106       |
| 0622 | Descriptor hash mismatch refuses load                          | high     | E106       |
| 0623 | 10 000 process cycles under allocator trap (FFI path)          | high     | E106       |
| 0624 | Double-release audit trap in debug builds                      | high     | E106       |
| 0625 | ArcTable drains to zero on engine shutdown                     | high     | E106       |

## Definition of done

- `cargo test -p patches-integration-tests` green.
- `cargo test -p patches-integration-tests --features
  patches-alloc-trap/audio-thread-allocator-trap` green.
- Each test uses the real gain dylib built by the workspace
  (no mock plugin crate for the round-trip — the SDK smoke in
  E105 already covers inline-macro expansion).
