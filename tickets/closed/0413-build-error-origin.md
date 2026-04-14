---
id: "0413"
title: Add origin to BuildError enums
priority: medium
created: 2026-04-14
epic: E075
depends_on: ["0412"]
---

## Summary

Attach `origin: Option<Provenance>` to both BuildError enums so that
interpreter and engine-level failures can point at the DSL source that
triggered them.

## Acceptance criteria

- [ ] `patches-core/src/build_error.rs`: refactor to
  ```rust
  pub struct BuildError {
      pub kind: BuildErrorKind,
      pub origin: Option<Provenance>,
  }
  ```
  (or equivalent per-variant `origin` fields — pick the shape that
  minimises call-site churn; the wrapper struct is likely cleaner).
- [ ] Same for `patches-engine/src/builder.rs` BuildError.
- [ ] Helper: `BuildError::with_origin(self, Provenance) -> Self` for
      wrapping errors returned by module constructors.
- [ ] `patches-interpreter/src/lib.rs` is the primary wrap site:
      every error returned from a module constructor or validator is
      tagged with the provenance of the FlatModule / FlatConnection
      being processed.
- [ ] Engine-internal errors (`PoolExhausted`, `ModuleCreationError`
      without DSL origin) leave `origin: None`.
- [ ] Updated Display impls omit the chain (rendering is 0414's
      responsibility); Display prints the existing semantic message.
- [ ] All ~57 `BuildError::` call sites across ~9 files compile and
      pass tests.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean.

## Notes

- `patches-core` BuildError is returned by module implementations that
  have no access to provenance — they cannot set `origin`. The
  interpreter wraps.
- Consider whether `Option<Provenance>` or a sentinel "no origin"
  Provenance is cleaner. `Option` is more honest and plays well with
  rendering ("if let Some(origin) = ...").

## Risks

- Call-site churn is wide but mechanical. Do it in one commit per
  crate to keep diffs reviewable.
- Engine-level errors that *could* carry origin but historically don't
  — grep for places that lose span info today and attach where
  reasonable. Don't block this ticket on exhaustively finding all such
  sites; follow-up with issue notes.
