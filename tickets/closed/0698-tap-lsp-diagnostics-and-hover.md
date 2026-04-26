---
id: "0698"
title: LSP diagnostics, hover, and VS Code TextMate parity
priority: medium
created: 2026-04-26
---

## Summary

Surface every tap-related diagnostic from the pest validation passes
(ticket 0696) through the LSP diagnostic channel, and add hover
support for tap components, qualified parameter keys, and tap names.
Add a TextMate grammar fallback to the VS Code extension so `~` and
tap-type tokens colour correctly without LSP attached.

## Acceptance criteria

- [ ] LSP diagnostics: every error produced by 0696 surfaces in
  `patches-lsp` with severity, message, and span. Manual smoke test
  in VS Code: typing each invalid form produces a red squiggle with
  the expected message.
- [ ] Hover on a tap component name (`meter`, `osc`, `spectrum`,
  `gate_led`, `trigger_led`): markdown popup describing what the
  pipeline does + parameter list with units and defaults. Static
  table indexed by component name.
- [ ] Hover on a qualified parameter key (`meter.window`): popup
  showing which component, the parameter's unit, and default value.
- [ ] Hover on an unqualified key in a simple tap: same content as
  the qualified-key hover, resolved through the single component.
- [ ] Hover on a tap name (the first identifier inside `~...(...)`):
  popup showing the upstream cable expression that feeds it (from
  provenance) and the components it dispatches to.
- [ ] VS Code extension: `patches-vscode/syntaxes/*.tmLanguage.json`
  (or equivalent) updated to recognise `~` and tap-type tokens.
  TextMate is the fallback before LSP boots; don't try to validate
  here.
- [ ] Optional: snippet for `~meter(<name>, window: <ms>)` registered
  in the VS Code extension. Not blocking.
- [ ] `cargo test -p patches-lsp` green; manual verification in VS
  Code recorded in the ticket on close.

## Notes

Diagnostics half is mechanical — the validation pass in 0696 already
produces the right diagnostic shape; LSP just forwards. The LSP
already runs pest alongside tree-sitter (memory:
`project_lsp_expansion_hover`); this ticket adds tap diagnostics to
the same channel.

Hover content lives as a static table in the LSP crate, indexed by
component name. Keep the markdown short — one-line description plus a
parameter table. Link to the ADR for the long form.

Tap name hover needs provenance from the desugarer (ticket 0697) to
identify the upstream cable. If 0697 hasn't shipped when this ticket
starts, defer the tap-name hover variant and ship the others;
diagnostics + component/parameter hover are independent of 0697.

## Cross-references

- ADR 0054 §1 — component set, qualifier rules.
- ADR 0055 §1 — bringup sequence (DSL changes).
- Memory: `project_lsp_expansion_hover`, `project_lsp_roadmap`.
- E118 — parent epic.
