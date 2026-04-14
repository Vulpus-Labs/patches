---
id: "0414"
title: Render expansion chains in player, LSP, and CLAP host
priority: medium
created: 2026-04-14
epic: E075
depends_on: ["0413"]
---

## Summary

Surface the provenance chain to users. Player and CLAP host print
human-readable error traces; LSP maps the chain into
`DiagnosticRelatedInformation`.

## Acceptance criteria

### patches-player

- [ ] On `BuildError` with `Some(origin)`, print:
  ```
  error: <kind message>
    --> <file>:<line>:<col>     <-- origin.site
    expanded from <file>:<line>:<col>  <-- origin.expansion[0]
    expanded from <file>:<line>:<col>  <-- origin.expansion[1]
    ...
  ```
- [ ] SourceId → PathBuf resolution uses the `SourceMap` carried
      alongside the loaded patch. Line/column computed from the
      source text.
- [ ] Pattern mirrors `loader.rs` include-chain rendering for
      consistency.

### patches-lsp

- [ ] `Diagnostic` in `patches-lsp/src/analysis.rs` gains a
      `related: Vec<(Span, String)>` field.
- [ ] For diagnostics that have provenance (from `BuildError` after
      0413, or from expand-time errors after 0411), populate `related`
      with one entry per expansion span, message "expanded from here".
- [ ] LSP server (`patches-lsp/src/server.rs`) converts `related`
      into `DiagnosticRelatedInformation` on the emitted LSP
      `Diagnostic`.
- [ ] Test: an LSP integration test on a two-level nested template
      with a known error asserts `relatedInformation.len() == 2`.

### patches-clap

- [ ] CLAP plugin's error surfacing path prints the chain (same
      formatter as the player, factored into a shared helper if
      convenient).

### Shared

- [ ] Factor rendering into a helper (likely in `patches-dsl` or a
      new `patches-diagnostics` module) taking `&Provenance` and
      `&SourceMap`. Player, CLAP, and LSP formatter all call it.
- [ ] Golden-snapshot test of player stderr for a known three-level
      failure.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean.

## Notes

- `DiagnosticRelatedInformation` is the LSP primitive designed for
  exactly this: a diagnostic at location A with additional notes at
  locations B, C, D. Editors render these as clickable breadcrumbs.
- Keep the player's output terse by default; consider a `--trace` or
  verbosity flag if the chain gets long, but don't add the flag until
  we see real patches produce unreadable output.

## Risks

- LSP clients vary in how they render `relatedInformation`. Test in
  the project's VS Code extension (`patches-vscode/`) specifically.
- If CLAP host has a limited error-surfacing channel (log line only),
  we may need to collapse the chain into a single formatted string.
