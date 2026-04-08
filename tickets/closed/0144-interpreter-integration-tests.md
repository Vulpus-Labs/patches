---
id: "0144"
title: Interpreter integration tests
priority: medium
created: 2026-03-19
epic: E029
depends_on: ["0143"]
---

## Summary

Add integration tests that exercise the full DSL compilation pipeline:
`parse` → `expand` → `build`. Tests should use the existing `.patches`
fixture corpus in `patches-dsl/tests/fixtures/` together with
`patches_modules::default_registry()`.

## Acceptance criteria

- [ ] Tests live in `patches-integration-tests/tests/dsl_pipeline.rs` (or
  a new file in that crate; follow the existing pattern).
- [ ] At minimum, tests cover:
  - **Flat patch round-trip**: parse `simple.patches`, expand, build; assert
    the resulting `ModuleGraph` contains the expected node IDs and that the
    expected connection exists.
  - **Template expansion**: parse `voice_template.patches`, expand, build;
    assert namespaced node IDs (e.g. `v1/osc`) appear in the graph.
  - **Nested templates**: parse `nested_templates.patches`, expand, build;
    assert deep namespacing works end-to-end.
  - **Unknown type error**: supply a `FlatPatch` with an unregistered type
    name; assert `build` returns `Err(InterpretError)` with a non-empty
    message.
  - **Unknown port error**: supply a `FlatPatch` with a connection referencing
    a non-existent port name; assert `build` returns an error.
- [ ] All tests are `#[test]` (no `#[ignore]`) — no audio hardware required.
- [ ] `cargo test -p patches-integration-tests` and `cargo clippy -p
  patches-integration-tests` pass with no warnings.

## Notes

The fixture patches reference module type names like `Osc`, `AudioOut`, `Vca`,
etc. These must match the `module_name` values registered in
`default_registry()`. If a fixture uses a type name that doesn't exist in
`patches-modules`, either update the fixture or note the mismatch as a
follow-up.
