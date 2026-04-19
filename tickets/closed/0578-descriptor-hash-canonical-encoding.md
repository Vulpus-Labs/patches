---
id: "0578"
title: Stable descriptor_hash with canonical byte encoding
priority: high
created: 2026-04-19
---

## Summary

Replace the `descriptor_hash` stub from 0577 with a stable
64-bit digest computed over a canonical byte encoding of the
descriptor shape. Used at load time (Spike 7) to reject
host/plugin descriptor drift; must therefore be reproducible
byte-for-byte across runs, machines, and compiler versions.

Encoding (all integers little-endian):

1. `u32` param count.
2. For each parameter in canonical order
   (`(name, indexed_position)`):
   - `u32` name length, UTF-8 bytes.
   - `u8` kind tag (`Float=0, Int=1, Bool=2, Enum=3, File=4,
     FloatBuffer=5`).
   - `u32` index (0 for scalar params).
   - For `Enum`: `u32` variant count, then per variant:
     `u32` length + UTF-8 bytes.
3. `u32` input port count, `u32` output port count.
4. Per port in declared order: `u32` name length + UTF-8 bytes,
   `u8` kind (mono/poly).

Digest: SHA-256 of the stream, truncated to the low 8 bytes as
`u64` little-endian.

## Acceptance criteria

- [ ] `compute_layout` populates `descriptor_hash` with the real
      digest.
- [ ] Deterministic: same descriptor encoded on x86_64 and
      aarch64 produces the same hash (verified via unit test using
      a fixed expected constant per fixture descriptor).
- [ ] Changing any one of: a param name, kind, variant name,
      variant order, port name, or port kind changes the hash.
- [ ] Reordering descriptor iteration does not change the hash
      (canonical ordering absorbs it).
- [ ] SHA-256 comes from an already-permitted dep, or a new dep
      is approved before adding. Prefer `sha2` if needed.
- [ ] `cargo clippy` clean.

## Notes

Depends on 0577. Must not use `DefaultHasher`, `Hash` derives, or
`HashMap` iteration — these are permitted to vary across runs.
