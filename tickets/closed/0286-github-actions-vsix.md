---
id: "0286"
title: GitHub Actions workflow for VSIX release builds
priority: high
created: 2026-04-08
---

## Summary

Create a GitHub Actions workflow that builds platform-specific `.vsix` packages
and attaches them to a GitHub Release. Triggered by pushing a version tag
(e.g. `v0.1.0`).

## Acceptance criteria

- [ ] `.github/workflows/release-vsix.yml` exists
- [ ] Workflow triggers on `push: tags: ['v*']`
- [ ] Matrix strategy: `macos-latest` (arm64), `macos-13` (x64), `ubuntu-latest` (x64)
- [ ] Each job: installs Rust, Node.js, builds `patches-lsp`, runs `npx @vscode/vsce package --target <target>`
- [ ] All `.vsix` artifacts are uploaded to the GitHub Release created by the tag
- [ ] Workflow uses `gh release create` or `softprops/action-gh-release` to publish

## Notes

- Building natively on each runner avoids cross-compilation headaches (no need
  for cross-linkers or foreign C toolchains).
- `macos-latest` is arm64 (M-series) on GitHub Actions; `macos-13` is the last
  Intel runner.
- The `npm ci` + `npx tsc` step can be shared across jobs or done once and
  cached — not critical for a first pass.
- Windows job is deferred to T-0289.
