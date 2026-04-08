---
id: "0262"
title: VS Code extension scaffold with syntax highlighting
priority: high
created: 2026-04-07
---

## Summary

Create a minimal VS Code extension that provides TextMate syntax highlighting
for `.patches` files and connects to the `patches-lsp` binary as an LSP client.
This is the test harness for all subsequent LSP work.

## Acceptance criteria

- [ ] `patches-vscode/` directory with `package.json`, `extension.ts`,
      `tsconfig.json`, and `language-configuration.json`.
- [ ] `package.json` declares language `patches` with file extension `.patches`,
      activation event `onLanguage:patches`, and LSP client configuration.
- [ ] `language-configuration.json` defines comment markers (`#`), bracket
      pairs, and auto-closing pairs.
- [ ] TextMate grammar (`patches.tmLanguage.json`) highlights: keywords
      (`module`, `template`, `patch`, `enum`, `in`, `out`, `true`, `false`),
      comments (`#`), string literals, numeric literals (including unit suffixes
      `hz`, `khz`, `db`), note literals, arrows (`->`, `<-`), param references
      (`<ident>`), module references (`$`), and type names after `:`.
- [ ] `extension.ts` spawns `patches-lsp` (found on `$PATH` or via a config
      setting) and connects via stdio using `vscode-languageclient`.
- [ ] F5 in VS Code opens an Extension Development Host with syntax highlighting
      active on `.patches` files.
- [ ] `npm install` and `npm run compile` succeed.

## Notes

- No bundling, packaging, or marketplace publishing.
- The LSP binary won't exist yet when this ticket starts — the extension should
  handle the binary being absent gracefully (log a message, no crash).
- Epic: E048
