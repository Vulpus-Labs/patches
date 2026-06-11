---
id: "0994"
title: FfiBytes must round-trip Vec capacity (UB on dealloc)
priority: high
created: 2026-06-11
---

## Summary

`FfiBytes::from_vec` (`patches-ffi-common/src/types.rs:59-62`) captures only
`ptr` and `len` from a `ManuallyDrop<Vec<u8>>`, discarding capacity.
`FfiBytes::reclaim` (`types.rs:86-92`) rebuilds with
`Vec::from_raw_parts(ptr, len, len)`. Any source `Vec` with
`capacity > len` — the normal case after `serialize_module_descriptor`,
`String::into_bytes`, or partially-filled `with_capacity` buffers — is
deallocated with the wrong `Layout`. That is undefined behavior per the
`GlobalAlloc` safety contract; current allocators tolerate it by size-class
accident.

## Acceptance criteria

- [x] `FfiBytes` carries `cap: usize`; `from_vec` records `v.capacity()`,
      `reclaim` passes it to `Vec::from_raw_parts`.
- [x] All construction/consumption sites updated (struct is `repr(C)` —
      this is an ABI break; `ABI_VERSION` bumped 12 → 13 so stale plugins
      refuse to load). All sites construct via `from_vec`/`empty`, so no
      raw struct literals needed updating.
- [x] Test: `ffi_bytes_capacity_gt_len_round_trip` round-trips a
      `Vec::with_capacity(64)` filled to 3 through `from_vec`/`reclaim`
      and asserts cap preservation; documented Miri invocation in the test
      doc comment.
- [x] `empty()` and null-pointer paths unchanged (still `ptr=null,
      len=0, cap=0`).

## Resolution

`patches-ffi-common/src/types.rs`: added `cap` field, updated
`from_vec`/`empty`/`reclaim`, bumped `ABI_VERSION` to 13 with a v13 doc
note. Closed under **E163**.

## Notes

Part of **E163**. Alternative considered: `shrink_to_fit` before capture —
rejected; it can still leave `capacity > len` (shrink is a request, not a
guarantee) and hides the contract instead of encoding it.
