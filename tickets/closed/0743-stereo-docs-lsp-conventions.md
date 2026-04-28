---
id: "0743"
title: Docs, LSP hover, and CLAUDE.md updates for stereo cables
priority: low
created: 2026-04-27
---

## Summary

Wrap up E125 with documentation, LSP hover, and convention updates.

## Acceptance criteria

- [ ] CLAUDE.md "Port naming conventions" updated: `_left`/`_right`
      retired for symmetric stereo modules, kept only for
      semantically-asymmetric pairs.
- [ ] mdBook manual: cable-kind reference gains a Stereo entry; tap
      reference adds `stereo_meter`; broadcast coercion documented.
- [ ] LSP hover renders `Stereo` cable kind and `stereo_meter` tap type.
- [ ] LSP go-to-definition follows broadcast cables to the mono source
      with no extra hop.
- [ ] ADR 0054 gets a "Superseded in part by ADR 0059" header note for
      §3, §4 (single Tap module, source-order slot mapping, identity
      tuple).

## Notes

Run `tools/align-tables.py` on edited markdown.
