---
id: "0272"
title: Context-sensitive completions
priority: high
created: 2026-04-07
---

## Summary

Implement `textDocument/completion` with context-sensitive suggestions driven
by the cursor's position in the CST and the stable semantic model.

## Acceptance criteria

- [ ] After `module name :` — complete with registered module type names and
      template names from the current file.
- [ ] Inside a shape block `(` — complete with shape argument names
      (`channels`, `length`, `high_quality`).
- [ ] Inside a param block `{` — complete with parameter names from the
      module's resolved `ModuleDescriptor`. For indexed parameters, suggest the
      base name.
- [ ] After `module_name.` in a connection — complete with port names from the
      module's descriptor (output ports for the source side, input ports for
      the destination side).
- [ ] After `$.` in a template body — complete with the template's declared
      in/out port names.
- [ ] Completions use the stable semantic model, so they remain available even
      when the current file state has transient errors.
- [ ] Manual verification: type `module osc : ` in VS Code, see a completion
      list with module type names.

## Notes

- Determining cursor context requires inspecting the CST node at the cursor
  position and its ancestors. The tree-sitter tree is used for this, not the
  tolerant AST.
- Completion items should include `kind` (e.g. `Module`, `Property`, `Field`)
  for appropriate icons in the editor.
- Epic: E050
