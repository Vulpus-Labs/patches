---
id: "0835"
title: Sweep `.unwrap()`/`.expect()` in library code
priority: medium
created: 2026-05-08
epic: E139
---

## Summary

CLAUDE.md states: "No `unwrap()` or `expect()` in library code — use
proper error propagation." The 2026-05-08 style survey found ~8 violations
in non-test paths. None are imminent panic risks (each is defended by an
upstream invariant), but the policy exists so reviewers don't have to
re-derive each invariant from context.

## Sites

- [patches-core/src/cables/ports.rs:27,38,49,84,95,106](../../patches-core/src/cables/ports.rs#L27)
  — `expect_mono`/`expect_poly`/`expect_stereo`. Defended by planner-time
  type checking. Either rename to `*_or_panic` and document the planner
  guarantee on the method, or split into a fallible `try_as_mono` plus a
  thin panicking wrapper used only at audio-thread call sites where the
  planner guarantee is structural.
- [patches-core/src/modules/descriptor_template.rs:148-153](../../patches-core/src/modules/descriptor_template.rs#L148)
  — `axis_count` hides a panic in `find_map(...).unwrap_or_else(|| panic!(...))`.
  Return `Result<u32, MissingAxis>` and propagate.
- [patches-engine/src/execution_state.rs:52-64](../../patches-engine/src/execution_state.rs#L52)
  — `PtrArray::rebuild` `.expect()` on `resolve()` and `NonNull::new()`.
  Plan-rebuild path; should carry slot index in the error.
- [patches-dsl/src/expand/expander/passes.rs:96](../../patches-dsl/src/expand/expander/passes.rs#L96)
  — `.unwrap()` on `get` after conditional insert. Capture the
  freshly-inserted reference instead of relooking it up.
- [patches-manifest/src/lib.rs:28-34](../../patches-manifest/src/lib.rs#L28)
  — `bundled_manifest()` `.expect("...must deserialize")`. Bundled JSON is
  baked at build time; document the early-startup failure mode and leave
  the panic, or move deserialization behind a `LazyLock<Result<…>>` that
  callers can surface as a typed error.
- [patches-planner/src/builder/mod.rs:116-117](../../patches-planner/src/builder/mod.rs#L116)
  — `ParamState::new_for_descriptor` `.expect("pack_into failed")`. Public
  API; must return `Result<Self, BuildError>`.
- [patches-registry/src/registry.rs:117-118](../../patches-registry/src/registry.rs#L117)
  — `describe()` `.expect("template recorded at register time")`.
  Cross-method invariant. Either collapse `register` + `describe` so the
  template doesn't need re-fetching by name, or carry the template via
  argument.
- [patches-lsp/src/server.rs:102](../../patches-lsp/src/server.rs#L102)
  — `.expect("serialize watch registration")`. Trivial `?` substitute.

## Acceptance criteria

- [ ] Each site above either returns `Result` with a typed error, or
      panics through a method whose name and doc-comment make the panic
      contract explicit
- [ ] `rg '\.unwrap\(\)|\.expect\(' --type rust -g '!**/tests/**' -g '!**/test_support/**' -g '!**/*test*.rs'`
      across the surveyed crates returns no library-path hits not covered
      by an explicit safety-doc comment
- [ ] `just push` clean

## Notes

Out of scope: test code, `test_support` modules, examples, binaries
(`patches-player/src/main.rs` startup `.expect()`s are acceptable). Audio
callbacks are already panic-free; this ticket is about library API
surface.

Mutex `unwrap_or_else(PoisonError::into_inner)` is **not** in scope —
it is deliberate per ADR 0056.
