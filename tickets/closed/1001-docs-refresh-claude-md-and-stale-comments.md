---
id: "1001"
title: "Docs refresh: CLAUDE.md workspace drift, trigger-port doc kinds, stale comments"
priority: medium
created: 2026-06-11
---

## Summary

CLAUDE.md predates the planner/host/cpal extraction and now misdescribes
the workspace; several in-code comments describe removed designs. Doc
drift was the single most systemic finding of the 2026-06 review.

Confirmed drift:

- **Workspace layout lists 15 crates; workspace has 27 members.**
  Undocumented: patches-sdk, patches-diagnostics, patches-planner,
  patches-cpal, patches-host, patches-tools, patches-forbidden-edges,
  patches-manifest, patches-svg, patches-graph-json, patches-observation,
  patches-io-ring, patches-alloc-trap.
- **patches-engine described as "Patch builder, sound engine, CPAL
  integration"** — builder moved to patches-planner, CPAL to
  patches-cpal; engine keeps "temporary" re-exports
  (`patches-engine/src/lib.rs:25-30`) with no cleanup ticket.
- **patches-dsl "no audio or module dependencies (only pest)"** — it
  depends on patches-core (Cargo.toml:13).
- **patches-integration-tests "depends on core, engine, modules"** —
  actually 5 deps + 9 dev-deps.
- **Trigger-port doc-kind mismatch** in at least 4 modules: doc tables say
  `mono`, descriptors say `PortTemplate::trigger` (midi_drumset.rs,
  midi_to_cv.rs, host/clock.rs, pattern_player/mod.rs). Grep-driven
  sweep; extend the module doc standard to require kind accuracy.
- **Stale comments:** `// Drop closes the vizia window` in wry-based
  `gui_destroy` (`patches-clap/src/extensions.rs:513`); `emit.rs:50-62`
  tap-guard comment reads as TODO for a pass that landed (ticket 0697);
  `errors.rs:9` claims InterpretErrorCode shares `BN####` format (it's
  `RT####`); `OtaPoles::Two` doc claims feedback from stage 1, code feeds
  back from stage 3/4 (`patches-dsp/src/ota_ladder/mod.rs:44-45`).
- **Dangling ticket references:** tap.rs cites ticket 0740,
  extensions.rs cites 0825 — neither exists. Create or fix.
- **Repo hygiene:** delete
  `patches-planner/src/builder/tests/lock_in.rs.orig` (999-line E160
  leftover) and `patches-dsl/.DS_Store`; remove the dead `rtrb` dep from
  `patches-dsp/Cargo.toml:19` (zero uses in src).

## Acceptance criteria

- [ ] CLAUDE.md workspace layout lists all members with one-line roles;
      crate descriptions corrected (engine, dsl, integration-tests);
      patches-sdk noted as the external SDK surface.
- [ ] Module doc standard amended: port Kind column must match descriptor
      port kind; trigger-port docs fixed via grep sweep.
- [ ] Stale comments above corrected; dangling ticket refs resolved
      (create the ticket or update the comment).
- [ ] `.orig`, `.DS_Store`, dead `rtrb` dep removed.
- [ ] Engine planner re-export cleanup gets its own tracking ticket if
      not folded in here (downstream imports in patches-integration-tests
      still go through patches_engine).

## Notes

Mechanical but broad; good candidate for a single pathspec-scoped commit
per area (docs, comments, hygiene) rather than one blob.

## Resolution (2026-06-11)

- **CLAUDE.md workspace layout** rewritten to list all 27 workspace crates
  (was 15) with one-line roles, including patches-planner / patches-cpal /
  patches-host / patches-observation / patches-io-ring / patches-alloc-trap
  / patches-svg / patches-graph-json / patches-diagnostics / patches-sdk /
  patches-manifest / patches-forbidden-edges / patches-tools. Prose fixed:
  patches-dsl depends on pest + patches-core; patches-dsp is the pure-DSP
  leaf (no rtrb — enforced by forbidden-edges); engine vs planner vs cpal
  split described; patches-sdk noted as the external SDK surface;
  patches-engine's temporary planner re-exports noted.
- **Module doc standard** amended: the Kind column must match the
  descriptor's port kind (trigger / stereo / poly / mono). Trigger-port
  doc sweep fixed `mono`→`trigger` in midi_drumset, midi_to_cv, host/clock,
  host/host_transport, modulators/quant (drumset table realigned).
  `trigger_sync_conv` was a false positive (two modules with opposite
  in/out kinds) — left correct.
- **Stale comments** corrected: gui_destroy "vizia"→"wry webview";
  descriptor_bind/errors.rs BN#### vs RT#### claim fixed; emit.rs tap-guard
  "until that pass lands"→"landed, defensive"; OtaPoles::Two feedback doc
  (feedback is always from stage 3, only the output tap differs).
- **Dangling ticket refs**: tap.rs 0740 reworded to "future work (no ticket
  filed)"; extensions.rs `TODO(0825?)` repointed to new ticket 1003
  (params_flush data race).
- **Hygiene**: deleted lock_in.rs.orig (tracked) and patches-dsl/.DS_Store
  (untracked); fixed the `.gitignore` `.DS_STORE`→`.DS_Store` case bug;
  removed the dead `rtrb` dep from patches-dsp/Cargo.toml.
- **Engine re-export cleanup**: filed as tracking ticket 1004 (not folded
  in — it touches downstream imports); in-code comment points at it.

Affected crates build; clippy clean on patches-modules / patches-dsp;
trigger-doc sweep returns clean.
