---
id: "0533"
title: Split patches-lsp analysis/tests.rs by category
priority: medium
created: 2026-04-17
epic: E090
---

## Summary

[patches-lsp/src/analysis/tests.rs](../../patches-lsp/src/analysis/tests.rs)
is 785 lines. Tests cover several analysis-pipeline axes: source-text
scanning, dependency graph construction, descriptor lookup for known
modules, template-instance port resolution, and parameter-name /
value validation.

## Acceptance criteria

- [ ] Convert to stub `src/analysis/tests.rs` declaring a submodule
      tree under `src/analysis/tests/`.
- [ ] Category split (final naming the ticket's call):
      - `scan.rs` — `scan_no_templates`, `scan_with_templates`,
        template-scan helpers
      - `deps.rs` — `dep_no_templates`, `dep_chain`, `dep_cycle`,
        `dep_independent_templates`
      - `descriptors.rs` — `descriptors_for_known_modules`,
        `diagnostic_for_unknown_module`, `template_instance_uses_template_ports`
      - `validation.rs` — parameter-name / value / `polylowpass_*`
        validation tests
- [ ] Shared fixtures (`parse`, `analyse_source` helpers) in
      `tests/mod.rs` or a `tests/support.rs`.
- [ ] `cargo test -p patches-lsp` passes with the same test count.
- [ ] `cargo build -p patches-lsp`, `cargo clippy` clean.

## Notes

E090. No test logic edits.
