---
id: "1002"
title: "Guard gaps: forbidden-edges dsp rule, smoke-tier dedup, planner property no-ops"
priority: medium
created: 2026-06-11
---

## Summary

Three places where a stated invariant has no automated guard:

1. **patches-forbidden-edges doesn't guard the patches-dsp leaf rule.**
   CLAUDE.md's most load-bearing layering claim — patches-dsp has no
   patches-core/CPAL/serde footprint — is unenforced; the `FORBIDDEN`
   array (`patches-forbidden-edges/src/main.rs:17-22`) and `LEAF_CRATES`
   list contain no patches-dsp rule. Currently true by luck.
2. **Smoke tier is redundant with push.** `Justfile:33-35` re-runs
   integration-tests/clap/lsp tests that `push`'s
   `cargo test --workspace --tests` already covers, with no extra flags.
   ADR 0067 says smoke = push + *extra* expensive suites. The natural
   extra: `--features audio-thread-allocator-trap` on the integration
   suite (see ticket 0997).
3. **Planner property tests stub the regression class ADR 0081 cited.**
   `patches-planner/tests/properties.rs:618-624` — `ChangeParam` /
   `ChangeStructural` history-replay arms are no-ops pending a
   ModuleGraph mutate-in-place API; param-diff classification
   (`classify_nodes`) goes property-untested.

## Acceptance criteria

- [ ] forbidden-edges fails on any patches-dsp dep beyond rtrb (allowlist
      form, so additions are deliberate), plus serde/cpal/patches-core
      named explicitly.
- [ ] Smoke tier de-duplicated and made a true superset: at minimum the
      alloc-trap-feature integration run; document what each smoke step
      adds over push.
- [ ] Planner: either the ModuleGraph mutation API lands and the two
      property arms exercise real edits, or a focused fixture-based test
      covers `classify_nodes` param-diff paths in the interim; the no-op
      arms stop consuming proptest budget silently (log or remove until
      real).
- [ ] Bonus if cheap: pin `rust-toolchain.toml` to a specific stable
      (known CI-vs-local clippy drift, see memory/ADR 0067 context) —
      decide and record either way.

## Notes

Item 3 references tickets 0982/0983 comments in the property suite;
reconcile with whatever their current state is before duplicating work.

## Resolution (2026-06-11)

1. **forbidden-edges patches-dsp leaf rule** — added a `DEP_ALLOWLIST`
   mechanism to `patches-forbidden-edges`: a crate's *normal* (non-dev,
   non-build) direct deps must be a subset of an explicit allowlist.
   `patches-dsp`'s allowlist is `["rtrb"]`, so any `patches-core` / CPAL /
   serde / etc. dep fails the check. (rtrb itself is dead and removed in
   ticket 1001; the allowlist keeps it permitted per CLAUDE.md's
   documented "lock-free ring buffers" design, so 1001's removal keeps the
   check green — zero deps ⊆ {rtrb}.) `cargo run -p patches-forbidden-edges`
   green.
2. **Smoke tier** — push already runs `cargo test --workspace --tests`
   (run-push.sh), so smoke's integration/clap/lsp re-runs were pure
   duplication. Replaced the smoke body with the one genuine extra: the
   integration suite under `--features audio-thread-allocator-trap` (the
   only tier that arms the audio-thread alloc trap — also ticket 0997
   item 3). Justfile comment documents what smoke adds over push.
3. **Planner property no-ops** — chose the interim fixture path (no
   ModuleGraph mutate-in-place API yet). The param-removal classification
   was *already* covered
   (`classify_surviving_removed_param_produces_diff_with_default`); the
   genuine gap was structural-change → Install, now covered by the new
   `classify_structural_changed_node_is_install`. Removed the silent
   `ChangeParam` / `ChangeStructural` no-op arms from the `Edit` enum,
   `arb_edit`, and `apply_history` so they stop consuming proptest budget;
   the module doc-comment records why and points at the fixture tests +
   0982/0983 for when a mutation API lands.
4. **Bonus: toolchain pin** — pinned `rust-toolchain.toml` from the
   floating `stable` to `1.95.0` (current local), eliminating the
   documented CI-vs-local clippy drift. Bump deliberately when adopting a
   newer stable.

`cargo test -p patches-planner` green; clippy clean on planner +
forbidden-edges.
