---
id: "0599"
title: Resolve `ParameterValue::File` off-thread; reject `File`/`String` at frame build
priority: high
created: 2026-04-20
depends_on: ["0595"]
---

## Summary

`ParameterKind::File` remains a legitimate descriptor declaration,
but `ParameterValue::File` must not reach the audio thread. The
planner resolves the file path to an `Arc<[f32]>`, inserts into
the runtime's `FloatBuffer` `ArcTable`, and substitutes a
`FloatBufferId` in the parameter map before frame pack. At frame
build, any remaining `File` (or the already-banned `String`) is a
planner bug.

## Scope

- Add a planner stage (control thread) that walks the parameter
  map for each instance, resolves `ParameterValue::File(path)` via
  the existing loader into an `Arc<[f32]>`, mints an id in the
  runtime's `FloatBuffer` `ArcTable`, and replaces the value with
  `ParameterValue::FloatBuffer(arc)` (so the existing pack path
  from Spike 3 encodes it into the tail slot as
  `FloatBufferId::pack(...)`).
- In `pack_into`, `debug_assert!` the value is not `File` or
  `String`; release build returns a planner error.
- Errors propagate as `BuildError`, not panics in release.
- Content dedup (same path twice → one `Arc`) is the planner's
  responsibility and should reuse whatever cache
  [patches-integration-tests/tests/file_params.rs](../../patches-integration-tests/tests/file_params.rs)
  exercises.

## Acceptance criteria

- [ ] Planner resolves `File` → `FloatBuffer` before `pack_into`.
- [ ] `pack_into` rejects `File` / `String` with a descriptive
      error; `debug_assert!` in debug.
- [ ] Existing file-parameter integration tests still pass;
      modules receiving a file still see the resolved buffer.
- [ ] Regression test: injecting a `File` value into the
      pack input produces the expected error (release) /
      panic (debug).
- [ ] `ParameterValue::File` still compiles — it remains
      constructible off-thread (DSL / interpreter / planner
      input) — it just can't survive frame build.

## Non-goals

- Removing `ParameterKind::File` from descriptors.
- Growth of the `FloatBuffer` `ArcTable` (spike 6).
- Deduplication policy changes beyond what's already present.
