---
id: "0449"
title: Split patches-lsp analysis.rs into phase modules
priority: medium
created: 2026-04-15
---

## Summary

`patches-lsp/src/analysis.rs` is 2155 lines and interleaves seven
distinct phases: info-type definitions, shallow scan, dependency
resolution, descriptor instantiation, body validation, tracker
validation, symbol indexing, and top-level orchestration. The work is
largely mechanical sifting/translation/aggregation, but the lack of
module boundaries makes it hard to see where pure AST→model
translation ends and diagnostic emission begins. Split into a module
directory so translation and validation live in separate files.

## Acceptance criteria

- [ ] `patches-lsp/src/analysis.rs` replaced by `patches-lsp/src/analysis/`
      containing:
  - `types.rs` — `ModuleInfo`, `ShapeValue`, `TemplateInfo`,
    `TemplateParamInfo`, `PortInfo`, `PatternInfo`, `SongInfo`,
    `SongCellInfo`, `DeclarationMap`.
  - `scan.rs` — `shallow_scan`, `extract_modules`, `extract_type_refs`,
    `make_key`, `ScopeKey`.
  - `deps.rs` — `DependencyResult`, `resolve_dependencies`.
  - `descriptor.rs` — `ResolvedDescriptor`, `instantiate_descriptors`,
    `dedup_port_names`, `format_port_labels`, `extract_channel_aliases`,
    `build_module_shape`.
  - `validate.rs` — `analyse_body`, `validate_body`,
    `validate_module_params`, `validate_connection`,
    `validate_port_ref_as_output`, `validate_port_ref_as_input`,
    `analyse_tracker`, `analyse_tracker_modules`.
  - `symbols.rs` — `collect_definitions`, `collect_references`,
    `collect_body_refs`, `collect_port_ref_refs`, `collect_param_refs`,
    `collect_value_param_refs`.
  - `mod.rs` — `SemanticModel`, `analyse`, `analyse_with_env`,
    re-exports used by the rest of `patches-lsp`.
- [ ] Public surface (items referenced from other `patches-lsp`
      modules) unchanged; only internal paths move.
- [ ] Existing tests migrate to `mod.rs` or split per phase; no test
      coverage lost.
- [ ] `cargo build`, `cargo test -p patches-lsp`, `cargo clippy` clean.

## Notes

Seen during E082 review. No behavioural change — pure reorganisation.
The split makes the transformation/validation boundary explicit:
`scan` + `deps` + `descriptor` + `symbols` are pure AST→model
translation; `validate` is the only module that emits diagnostics;
`mod.rs` orchestrates.

Not strictly part of E082's contract-enforcement theme, but sits
alongside it as structural cleanup in the same crate. Add to the epic
or leave standalone — either works.
