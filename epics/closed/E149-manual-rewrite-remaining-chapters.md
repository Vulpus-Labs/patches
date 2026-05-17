---
id: E149
title: Manual rewrite — remaining chapters
status: closed
created: 2026-05-17
---

## Goal

Complete the manual rewrite started 2026-05-17. Part I (Introduction
+ Mental model), the README, and the new `SUMMARY.md` structure are
landed; 21 chapter stubs currently read "To be written". This epic
tracks filling those stubs and auditing three reused chapters.

## Background

The old manual centred live-coding as the headline workflow. The
rewrite treats Patches as a general-purpose modular instrument with
three peer workflows — CLAP plugin in a DAW, standalone player with
external MIDI, self-contained sequenced piece — and positions the
edit-reload cycle as a dev REPL, not a performance feature.

### Source material

Deleted as misleading (recover from git history):

- `docs/src/building-a-patch.md` → harvest for DSL chapters
- `docs/src/anatomy-of-a-synth.md` → harvest for synth-voice chapter
- `docs/src/live-coding.md` → harvest for edit-reload chapter
- `docs/src/abi/descriptor-schema.md` + `abi/wire-formats.md` → fold
  into the new ABI chapter

Existing chapters reused, needing audit + light update:

- `docs/src/dsl-reference.md`
- `docs/src/implementing-modules.md`
- `docs/src/engine-internals.md`

Module reference (`docs/src/modules/*.md`) is out of scope — it's
generated from in-source doc comments per the module documentation
standard in CLAUDE.md.

### New framing rules

- No single workflow is privileged. CLAP, standalone-with-MIDI, and
  self-contained-with-tracker are equal peers.
- Edit-reload = REPL for the dev cycle. Live performance is also
  possible, not headline.
- All distributed binaries are unsigned; install chapters must cover
  OS-specific workarounds.
- Install chapters cover both prebuilt artefacts and build-from-source.

## Tickets

- [0901 — Installation chapters](../../tickets/open/0901-manual-installation-chapters.md)
- [0902 — Basic operation chapters](../../tickets/open/0902-manual-basic-operation-chapters.md)
- [0903 — DSL chapters](../../tickets/open/0903-manual-dsl-chapters.md)
- [0904 — Patch authoring chapters](../../tickets/open/0904-manual-patch-authoring-chapters.md)
- [0905 — Extending Patches chapters](../../tickets/open/0905-manual-extending-chapters.md)
- [0906 — Internals chapters](../../tickets/open/0906-manual-internals-chapters.md)
- [0907 — Appendices](../../tickets/open/0907-manual-appendices.md)

## Acceptance

- All 21 chapter stubs in `docs/src/` replaced with substantive
  content.
- Three reused chapters audited for accuracy and tone consistency.
- `mdbook build docs/` succeeds with no broken links.
- No "To be written" placeholder remains in the rendered manual.
- README and Part I (introduction, mental-model) not retouched
  unless an inconsistency surfaces during downstream writing — flag
  rather than silently edit.
