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

- [ ] Expander collects all host control blocks, groups by output
      cable type (knob/slider/toggle → Mono+Audio; trigger →
      Mono+Trigger; ADR 0057 §2), sorts each group alphabetically,
      assigns slot indices 0..N−1 within the group.
- [ ] Synthesises up to two module instances:
      `~host_control : HostControl(channels: N)` for audio-shaped
      controls and `~host_control_trigger : HostControlTrigger(channels: M)`
      for trigger-shaped controls. Empty groups emit no instance.
- [ ] Bare-name references in cables rewrite to
      `~host_control.out[<name>]` (audio kinds) or
      `~host_control_trigger.out[<name>]` (trigger kind), based on
      the declared kind of the referenced block.
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
