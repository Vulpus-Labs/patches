---
id: "0287"
title: Release notes template with unsigned-binary instructions
priority: medium
created: 2026-04-08
---

## Summary

Create a release notes template (or a section in README) with clear
instructions for installing the `.vsix` and dealing with unsigned binaries on
macOS and Windows.

## Acceptance criteria

- [ ] Template covers: download `.vsix`, install via `code --install-extension <file>.vsix`
- [ ] macOS section: `xattr -d com.apple.quarantine` command, with the typical extension install path (`~/.vscode/extensions/vulpus-labs.patches-vscode-*/server/patches-lsp`)
- [ ] macOS section: alternative System Settings > Privacy & Security > "Allow Anyway" flow
- [ ] Windows section: SmartScreen "Run anyway" flow, or right-click > Properties > Unblock
- [ ] Note that `patches.lsp.path` setting can point to a separately-built binary as a workaround

## Notes

- Keep instructions concise — users who install unsigned dev tools are generally
  comfortable with a terminal command.
- Consider adding the quarantine-removal hint to the extension's error message
  (already done in extension.ts for macOS; check if Windows needs similar).
