---
id: "0580"
title: Payload id newtypes with slot+generation encoding
priority: high
created: 2026-04-19
---

## Summary

Introduce `FloatBufferId` and `SongDataId` in `patches-ffi-common`
as `#[repr(transparent)]` u64 newtypes, with private constructors
and helpers to encode and decode `(generation: u32, slot: u32)`.
These are the handle types that cross the audio-thread boundary in
ADR 0045; they must be impossible to forge outside the crate and
cheap to copy.

## Acceptance criteria

- [ ] New module `patches-ffi-common::ids` defines
      `FloatBufferId(u64)` and `SongDataId(u64)` with
      `#[repr(transparent)]` and private fields.
- [ ] Associated helpers (crate-visible): `pack(generation, slot)
      -> Self`, `slot(self) -> u32`, `generation(self) -> u32`,
      `as_u64(self) -> u64`, `from_u64_unchecked(u64) -> Self`
      (crate-private, used by the ABI layer only).
- [ ] `Copy + Clone + Eq + Hash + Debug`; no `Default` (a
      default-constructed id would bypass the mint discipline).
- [ ] Unit tests: `pack` then `slot`/`generation` round-trip for a
      spread of values including `slot = 0`, `slot = u32::MAX`,
      `generation = 0`, `generation = u32::MAX`.
- [ ] `cargo clippy -p patches-ffi-common` clean.

## Notes

The types are payload-typed so buffer ids and song ids are not
interchangeable (ADR 0045 resolved design point 1). They deliberately
do not `impl Deref<Target = u64>`; callers that need the raw u64 for
the ABI go through `as_u64`. The `from_u64_unchecked` path exists
for the audio-thread decoder and is marked crate-private so plugin
code cannot reach it. A later spike will seal this further with a
feature gate for the decoder module.
