---
id: "0868"
title: Permit implicit `~name` host controls; demote knob/slider range fields
priority: medium
created: 2026-05-11
---

## Summary

Host-control decorators (`knob`/`slider`/`toggle`/`trigger`) currently force
users to write a declaration block before referencing a lane in a cable. For
knob/slider the block must include `low` and `high`, but these fields are
display-only metadata: CLAP publishes a fixed `[-1.0, 1.0]` automation range
and patches remap to musical units via the cable `-[bi(low, high)]->`
operator. Two ergonomic frictions follow:

1. `low`/`high` are required ceremony with no semantic teeth — they are not
   read at runtime or by CLAP param metadata.
2. Every `~name` cable endpoint needs a prior block, even when the user just
   wants a default knob.

Demote `low`/`high` to optional, and allow `~name` to introduce an implicit
knob lane on first use (range `[-1, 1]`, default knob behaviour).

## Acceptance criteria

- [ ] `validate.rs` no longer requires `low`/`high` for `knob`/`slider`.
      Toggle still requires `default`. Trigger unchanged.
- [ ] When a cable references `~name` and no `host_control_block` declared
      it, the expander synthesises an implicit `knob name {}` block. Same
      code path as an explicit empty `knob name {}` block — no parallel
      runtime path.
- [ ] Explicit and implicit forms produce identical `FlatPatch` output
      (round-trip test).
- [ ] LSP hover on an implicit `~name` still works (shows kind: knob,
      defaults).
- [ ] Existing patches with explicit `knob name { low: -1, high: 1 }`
      continue to validate and behave unchanged.
- [ ] Syntax corpus entry added for implicit-knob form (per repo policy).
- [ ] Manual updated: `docs/src/` host-control chapter reflects implicit
      form and optional range.

## Notes

Context: investigation in conversation 2026-05-11 confirmed that on the
audio thread only synthesised `slot_offset` and `kind` reach
`HostControl::process`. CLAP uses `default` for initial value; `low`/`high`
flow only into the manifest for UI consumers. Range remapping is the cable
operator's job.

Implicit kind is always `knob`. No inference from cable destination — users
wanting `trigger`/`toggle`/`slider` must declare explicitly.

`low`/`high` could be fully removed rather than demoted, since nothing
consumes them. Keeping them as optional documentation fields is the
conservative move — LSP hover renders them, and a future ticket could route
them into CLAP `display_value` formatting.

Related: `knob` and `slider` are runtime-identical (both `Smoothed`). A
follow-up could merge them or rename one to a pure alias; out of scope here.
