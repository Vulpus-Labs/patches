---
id: "0568"
title: patches-vscode settings schema and rescan command
priority: medium
created: 2026-04-19
epic: "E094"
---

## Summary

Expose module paths configuration and a rescan command through the
VSCode extension, wired to the LSP.

## Acceptance criteria

- [ ] `package.json` contributes a configuration setting
      `patches.modulePaths: string[]` with description and workspace
      scope.
- [ ] Setting changes propagate to the LSP via
      `workspace/didChangeConfiguration`.
- [ ] `package.json` contributes a command `patches.rescanModules`
      (palette title: "Patches: Rescan external module plugins").
- [ ] Command handler sends the `patches/rescanModules` custom request
      to the LSP, awaits the `ScanReport`, and shows a summary
      notification (`N loaded, M replaced, K skipped, errors: …`).
- [ ] Errors surfaced as a VSCode error notification listing each
      failed path.

## Notes

ADR 0044 §5. No UI for editing paths beyond the standard settings
panel — keeps the extension thin.
