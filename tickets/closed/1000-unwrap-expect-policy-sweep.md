---
id: "1000"
title: unwrap/expect policy sweep + policy decision on invariant panics
priority: medium
created: 2026-06-11
---

## Summary

CLAUDE.md states "No `unwrap()` or `expect()` in library code" absolutely.
The 2026-06 review found ~8 violations in production library code, most
guarding genuinely-true invariants. Two problems: the violations
themselves, and a policy that doesn't distinguish "documented invariant
that cannot fail" from "lazy error handling" — so greps for violations
return noise and the rule erodes.

Known sites (audio-thread-fence sites are E163's, excluded here):

- `patches-planner/src/builder/mod.rs:656` — `expect` in `build_draft`
  (ADR 0081 explicitly aims for structured `PlanError`/`BuildError`;
  convert to `BuildError::Internal`).
- `patches-core/src/cables/ports.rs:35-72` — `expect_mono/poly/stereo`
  panic paths on pub API.
- `patches-plugin-common/src/host_control_registry.rs:174,188,215` —
  three `expect`s on CLAP main thread (no halt machinery there).
- `patches-observation/src/observer.rs:275` — `spawn().expect`.
- `patches-observation/src/processor.rs:67-72` — `ProcessorId::index()`
  panics on pub `Spectrum`/`Scope` variants, reachable through
  `SubscribersHandle::read` with no guard. Split the enum or guard the
  read path — this one is an API trap, not just style.
- `patches-modules/src/midi/midi_delay.rs:114` — `unwrap` in
  `drop_oldest`, called from `process()` (audio path).
- `patches-profiling/src/timing_collector.rs:50,61,73` — mutex
  `unwrap`s (dev-only crate; lowest priority).
- `patches-core/src/registry/registry.rs:53-56` — `assert!` on empty
  template name at registration.

## Acceptance criteria

- [ ] CLAUDE.md policy amended: documented-invariant panics permitted only
      via a named mechanism (e.g. an `invariant!`/`expect_invariant!`
      macro or a standing `// INVARIANT:` comment form) so plain
      `unwrap()`/`expect()` greps stay meaningful. Decide and record.
- [ ] Each site above either converted to error propagation or migrated
      to the sanctioned invariant form; `ProcessorId::index()` resolved
      as an API fix (enum split or guarded read), not a comment.
- [ ] Fresh grep over library crates confirms no unsanctioned
      `unwrap`/`expect` outside tests.

## Notes

Audio-thread `expect`s inside the adoption path are handled by 0996; do
not double-fix. The planner site is the highest-value conversion (ADR
0081 already promises it).

## Resolution (2026-06-11)

**Mechanism** — added `patches_core::ExpectInvariant` (a trait giving
`.expect_invariant(msg)` on `Option`/`Result`, `#[track_caller]`). A
documented-invariant panic uses this named form so `\.unwrap(`/`\.expect(`
greps return only genuine violations; `assert!`/`panic!` remain sanctioned
for assertion-style checks. CLAUDE.md "General conventions" amended to
record the policy.

**Sites:**

- planner `build_draft` → `BuildError::InternalError` via `ok_or_else`/`?`
  (ADR 0081's promised structured error).
- `ports.rs` `expect_mono/poly/stereo` (×6, in + out) → `expect_invariant`
  citing the planner connection-time type check.
- `host_control_registry.rs` (×3, CLAP main thread) → `expect_invariant`.
- `ProcessorId::index()` → **API fix**: now returns `Option<usize>`
  (`None` for the `Spectrum`/`Scope` vector streams); the four
  `LatestValues` scalar methods guard with the `Option` (no-op / `0.0`).
  The pub-API panic trap is gone.
- `midi_delay::drop_oldest` audio-path `unwrap` → `let-else { return }`
  (panic-free regardless of the invariant).
- `observer.rs` `spawn().expect` → `.ok()` with graceful degradation
  (`thread: None`): a meters/scopes thread-spawn failure no longer panics
  the host; observation just goes dark.
- `timing_collector.rs` (×3, dev-only) mutex → `expect_invariant`.
- `registry.rs` `assert!` on empty template name — left as-is; `assert!`
  is a sanctioned invariant form (distinct from unwrap/expect for greps).

**Residual** — a fresh grep still finds ~100 library-code sites outside
tests, almost all grammar-guaranteed parser internals
(`pair.into_inner().next().unwrap()`) plus ~20 layout/algorithm invariants
the 2026-06 review did not flag. Converting that whole population is out of
proportion to this ticket's curated list and is tracked separately in
**ticket 1005**. The `de.rs` `self.expect(byte)` hits are a custom method,
not std `expect`.

`cargo test` + `clippy` green on all touched crates; downstream consumers
(engine/cpal/host/clap/player) build against the new `ProcessorId::index`
signature.
