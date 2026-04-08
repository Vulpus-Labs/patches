---
id: "0288"
title: Smoke-test script for installed .vsix
priority: medium
created: 2026-04-08
---

## Summary

A lightweight script or checklist to verify a `.vsix` package works end-to-end
after installation.

## Acceptance criteria

- [ ] Script or documented manual steps that: install the `.vsix`, open a `.patches` file, and verify syntax highlighting + LSP features (hover, completion, diagnostics) are working
- [ ] Can be run locally or as a CI step (headless VS Code via `@vscode/test-electron` or manual)
- [ ] Covers the "bundled binary not found" and "quarantine blocked" failure modes with expected error messages

## Notes

- Full automated VS Code integration testing is heavy; a manual checklist with
  a sample `.patches` file is fine for a first pass.
- The existing `examples/` directory has `.patches` files that can serve as test
  inputs.
