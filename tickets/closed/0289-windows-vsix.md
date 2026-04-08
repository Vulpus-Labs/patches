---
id: "0289"
title: Windows cross-compilation and .vsix packaging
priority: low
created: 2026-04-08
---

## Summary

Add a Windows x64 `.vsix` build to the release workflow.

## Acceptance criteria

- [ ] `windows-latest` job added to the release workflow matrix
- [ ] `cargo build --release -p patches-lsp --target x86_64-pc-windows-msvc` succeeds on the Windows runner
- [ ] `patches-vscode-win32-x64-*.vsix` is published to the GitHub Release
- [ ] Release notes include Windows SmartScreen / Unblock instructions

## Notes

- The `cc` crate should find MSVC automatically on the Windows runner.
- tree-sitter C code may need minor adjustments for MSVC (warnings-as-errors, etc.).
- Lower priority since the primary audience is macOS for now.
