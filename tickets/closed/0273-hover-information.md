---
id: "0273"
title: Hover information for modules and ports
priority: medium
created: 2026-04-07
---

## Summary

Implement `textDocument/hover` to display module and port metadata when the
user hovers over relevant tokens.

## Acceptance criteria

- [ ] Hovering over a module type name (e.g. `Osc` in `module osc : Osc`)
      shows the module's descriptor summary: input port names, output port
      names, and parameter names with their types and default values.
- [ ] Hovering over a port name in a connection (e.g. `sine` in `osc.sine`)
      shows the port kind (mono/poly) and index.
- [ ] Hovering over a template name (in a module declaration using a template
      type) shows the template's declared params (with types and defaults) and
      in/out ports.
- [ ] Hover content is formatted as markdown for readable rendering.
- [ ] Manual verification: hover over `Osc` in VS Code, see a panel with
      port and parameter information.

## Notes

- Hover uses the stable semantic model to look up descriptors.
- The hover position is resolved to a CST node, then matched against the
  semantic model by name.
- Epic: E050
