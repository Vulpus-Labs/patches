---
id: "0738"
title: Update example patches and integration tests for stereo ports
priority: medium
created: 2026-04-27
---

## Summary

Migrate every `.patches` example and DSL fixture that wires
`*_left`/`*_right` to use the collapsed stereo ports introduced in
0737. Where a mono source previously fanned into both halves of a
stereo input, rely on the broadcast coercion (0736) and keep one
cable.

## Acceptance criteria

- [ ] All files under `examples/` parse and bind under the new
      descriptors.
- [ ] DSL fixture corpus (`patches-dsl/tests/fixtures`) updated.
- [ ] `patches-integration-tests` green.
- [ ] `examples/drum_machine.patches` showcases stereo cables on the
      master bus.

## Notes

Run `tools/align-tables.py` after manual edits to fix any MD060 table
linting in updated docs. Do a grep for `_left|_right` across `examples/`
and `tickets/` to catch references in prose too.
