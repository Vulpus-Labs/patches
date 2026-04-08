---
id: "E054"
title: "VSIX distribution via GitHub Releases"
created: 2026-04-08
tickets: ["0285", "0286", "0287", "0288", "0289"]
---

## Summary

Ship platform-specific `.vsix` packages (macOS arm64, macOS x64, Linux x64,
Windows x64) as GitHub Release artifacts so users can install the Patches VSCode
extension with a bundled LSP binary. The binary is unsigned for now, so macOS
and Windows users need guidance on bypassing OS protections.

## Context

The extension and build script groundwork are already in place:

- `patches-vscode/src/extension.ts` resolves a bundled `server/patches-lsp`
  binary, falls back to user config / PATH, and shows a macOS quarantine hint
  on failure.
- `scripts/package-vsix.sh` compiles the LSP for a given target, copies the
  binary into `server/`, and runs `vsce package --target`.

What remains is cross-compilation setup, a GitHub Actions workflow, release
notes with unsigned-binary instructions, and a smoke test that the `.vsix`
actually works on each platform.

## Tickets

| ID   | Title                                                    | Priority | Depends on |
|------|----------------------------------------------------------|----------|------------|
| 0285 | Install Rust cross-compilation targets and verify builds | high     |            |
| 0286 | GitHub Actions workflow for VSIX release builds          | high     | 0285       |
| 0287 | Release notes template with unsigned-binary instructions | medium   | 0286       |
| 0288 | Smoke-test script for installed .vsix                    | medium   | 0286       |
| 0289 | Windows cross-compilation and .vsix packaging            | low      | 0285       |

## Definition of done

- `gh release create` (or the Actions workflow) publishes `.vsix` files for at
  least macOS arm64 and macOS x64.
- Release notes include clear instructions for removing the quarantine
  attribute on macOS and unblocking on Windows.
- A manual or scripted smoke test confirms the extension activates and the LSP
  responds to hover/completion in VS Code.
