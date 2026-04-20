---
id: "E099"
title: ADR 0045 spike 3 — ParamFrame, SPSC triplet, pack + view (shadow path)
created: 2026-04-19
depends_on: ["ADR 0045 spike 1", "ADR 0045 spike 2"]
tickets: ["0585", "0586", "0587", "0588", "0589", "0590"]
---

## Goal

Build the control-thread → audio-thread data-plane transport that
ADR 0045 section 3 specifies, wired alongside the existing
`ParameterMap` path as a **shadow**. After this epic:

- `ParamFrame` exists as an owned `Vec<u8>` carrying a packed
  scalar area plus a tail `u64` slot table for buffer ids, sized
  from a module instance's `ParamLayout`.
- `pack_into(layout, &ParameterMap, &mut ParamFrame)` encodes on
  the control thread with zero allocation after frame warm-up.
- `ParamView<'a>` reads the frame back via a perfect-hash
  (`ParameterKey` → slot) built at `prepare`, with `float` / `int`
  / `bool` / `enum_variant` / `buffer` accessors.
- A three-SPSC shuttle (`dispatch` / `cleanup` / `free`) recycles
  frame buffers per module instance, with back-pressure
  coalescing by `(module_idx, ParameterKey)`.
- The engine, in debug builds, encodes a `ParamFrame` for every
  in-process parameter update, decodes it through `ParamView`,
  and asserts field-by-field equality against the live
  `ParameterMap`. Production reads still go through
  `ParameterMap` — no audio-thread behaviour change, no trait
  signature change yet.
- No FFI wiring (spike 7). No audio-thread allocator trap yet
  (spike 4). No `String`/`File` removal (spike 5). No
  `ParamView`-only trait (spike 5).

Implements ADR 0045, spike 3. Depends on spike 1 (ParamLayout,
epic E097) and spike 2 (ArcTable, epic E098).

## Tickets

| ID   | Title                                                                   | Priority | Depends on |
| ---- | ----------------------------------------------------------------------- | -------- | ---------- |
| 0585 | `ParamFrame` buffer type with scalar area + tail u64 slot table         | high     | —          |
| 0586 | `pack_into` encoder: `ParameterMap` + `ParamLayout` → `ParamFrame`      | high     | 0585       |
| 0587 | `ParamView` reader with perfect-hash name lookup built at prepare       | high     | 0585, 0586 |
| 0588 | Three-SPSC frame shuttle with free-list recycling and coalescing        | high     | 0585       |
| 0589 | Shadow-path wiring in engine: encode + decode + debug equality assert   | high     | 0586, 0587, 0588 |
| 0590 | Property tests + no-alloc soak (10k cycles after warm-up)               | high     | 0589       |

## Definition of done

- `ParamFrame` owns a `Vec<u8>` whose length equals
  `layout.scalar_size + layout.buffer_slots.len() * 8`. Capacity
  sized at construction; length never changes after that.
- `pack_into` writes every scalar at its layout offset and every
  buffer id at its tail slot index. Missing keys in the
  `ParameterMap` use the descriptor default. No allocation after
  the frame is constructed.
- `ParamView::float/int/bool/enum_variant/buffer` are O(1) via a
  perfect-hash table built once per instance at `prepare` from
  the layout. No hashing collisions, no fallback path, no
  allocation per call.
- Three SPSCs per module instance use `rtrb` (already a workspace
  dep). The free-list is pre-filled to a caller-specified depth;
  dispatch coalescing holds at most one pending frame per
  `(module_idx, ParameterKey)` via a slot table ahead of the
  SPSC; newer updates overwrite pending frames (last-wins per
  ADR 0045 section 3).
- Engine shadow path: for every parameter update currently
  flowing through `ParameterMap`, the engine also packs a
  `ParamFrame`, constructs a `ParamView`, and in
  `debug_assertions` compares it against the `ParameterMap` the
  module sees, panicking on divergence. No production behaviour
  change; full existing test suite remains green.
- Property tests: random `ParameterMap` compatible with a random
  descriptor round-trips through `pack_into` → `ParamView`
  equally for every key. Shadow assert stays quiet across
  `cargo test` workspace-wide.
- No-alloc soak: after a warm-up phase that primes the free-list,
  10 000 iterations of pack → dispatch → consume → recycle
  allocate zero bytes (measured via a counting allocator in
  test-only code). Counted per module instance, not globally.
- `cargo build`, `cargo test`, `cargo clippy` clean. No
  `unwrap`/`expect` in library code. No new `Cargo.toml`
  dependency without prior sign-off — `rtrb` covers the SPSCs.

## Non-goals

- Removing `ParameterMap` from the `Module` trait signature
  (spike 5). Shadow path only; production still reads the map.
- Audio-thread allocator trap (spike 4) — will retro-validate
  this spike's no-alloc claim once landed.
- `ParameterValue::String`/`File` removal from the update path
  (spike 5). The shadow frame skips these variants and the
  debug-assert compares only variants the frame carries.
- `#[derive(ParamEnum)]` / `params_enum!` macro for typed enum
  access (spike 5). Spike 3 uses the existing `params_enum!`
  from E096 where it helps tests, no new macro work.
- FFI ABI surface. This spike is in-process only; the FFI loader
  keeps its JSON path until spike 7.
- Port bindings (`PortFrame` / `PortView`). Same transport shape
  but out of scope for this spike; lands alongside spike 5 or 7
  as appropriate.
- Growth of the ArcTable refcount map (spike 6).

## Notes

The shadow path is the load-bearing design choice: it lets us
ship and test the full transport — encoder, buffer, reader,
SPSCs, free-list recycling, coalescing — against the live
behaviour of every in-process module without a flag-day switch.
The production trait change is deferred to spike 5 precisely
because spike 3 has already proven equivalence at that point.
