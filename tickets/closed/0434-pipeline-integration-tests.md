---
id: "0434"
title: Pipeline integration tests across fail-fast and accumulate
priority: medium
created: 2026-04-15
---

## Summary

Extend `patches-integration-tests/tests/dsl_pipeline.rs` beyond the
current success-path coverage to exercise every stage transition
under both policies. Add an LSP integration harness that drives the
staged pipeline end-to-end against clean, syntax-broken,
structurally-broken, and binding-broken fixtures and asserts on the
aggregated diagnostics set.

## Acceptance criteria

- [ ] Fail-fast cases covered for stages 1–5: fixture input plus
      expected first-failing stage and error variant.
- [ ] Accumulate cases covered for LSP policy: fixtures that fail at
      stage 3a still produce stage 1/2 artifacts and the expected
      diagnostics aggregate.
- [ ] LSP integration harness drives a synthetic `DocumentWorkspace`
      through load → diagnostics publish and asserts on the published
      set.
- [ ] Tree-sitter fallback exercised by a syntax-broken fixture,
      verifying stages 4a–4b produced the expected shallow diagnostics.
- [ ] `cargo test` (workspace) clean.

## Notes

Depends on 0430, 0431, 0432, 0433. This ticket is what lets E078's
deferred LSP features (inlay hints, peek expansion, unused-output
diagnostics) assume a trustworthy bound graph — no silent drift
between what the tests promise and what handlers receive.
