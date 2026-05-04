---
id: "0808"
title: Expander synthesises ~host_control module from blocks
priority: high
created: 2026-05-04
epic: E135
depends_on: "0807"
---

## Summary

Lower host control declarations + bare-name references into a single
synthesised module instance, mirroring ADR 0054 §2 for taps.

## Acceptance criteria

- [ ] Expander collects all `HostControlDecl`s, sorts by name
      (alphabetical, ADR 0057 §3), assigns slot indices 0..N−1.
- [ ] Synthesises one `~host_control : HostControl(channels: N)`
      module instance with per-channel `name` and `slot_offset`
      params.
- [ ] Bare-name references in cables rewrite to
      `~host_control.out[<name>]`.
- [ ] `~` reserved-prefix rule enforced: user modules may not start
      with `~` (existing rule from ADR 0054 §2 — extend tests if
      needed).
- [ ] Adding/removing a host control changes the synthesised
      module's `channels` shape, triggering the existing size-change
      → drop+replace path. Renames / field-only changes preserve the
      shape.
- [ ] Empty case: zero host control declarations → no synthesised
      module emitted.
- [ ] Expander tests cover: alphabetical slot ordering, rename
      preserves shape, add/remove changes shape, bare-name reference
      rewrite, zero-declaration path.
- [ ] `just inner -p patches-interpreter -p patches-dsl` passes.

## Notes

- Slot ordering is recomputed independently both sides (audio
  module + CLAP plugin) from the same alphabetical input list. No
  cross-side state to keep in sync.
