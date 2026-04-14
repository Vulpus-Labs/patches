---
id: "0406"
title: Expander scope model and song-local pattern mangling
priority: high
created: 2026-04-14
---

## Summary

Extend the expander's `NameScope` to support the three-layer scope model
from ADR 0035 (song / template / file) with a parent chain. Inline
patterns defined inside a song are mangled using `QName::child` so their
emitted names are unique, and cell references inside that song resolve
to the correct `QName`.

## Acceptance criteria

- [ ] `NameScope` supports a parent link and holds song-local patterns
      and sections in addition to existing module/template data.
- [ ] Entering a song pushes a song scope whose parent is the enclosing
      scope (template or file).
- [ ] Inline `pattern` blocks inside a song emit as
      `QName { path: [song_name, ...], name: pattern_name }` — or
      extend whatever `QName::path` prefix the enclosing scope already
      provided.
- [ ] Top-level `section` blocks resolve against the invoking song's
      scope at expansion; same-name collisions within a scope are an
      error; different songs may define same-named song-local patterns
      without conflict.
- [ ] Scope-isolation tests: a pattern defined in song A is not
      resolvable from song B.
- [ ] Lane-count mismatch at a top-level section call site reports a
      clear expansion error.

## Notes

Depends on E073 (`QName`) and 0405. Split this from the flattening work
(0407) so scope and mangling can be tested independently.
