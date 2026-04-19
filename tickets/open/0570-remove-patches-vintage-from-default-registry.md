---
id: "0570"
title: Remove patches-vintage from default_registry
priority: high
created: 2026-04-19
epic: "E095"
---

## Summary

Once `patches-vintage` is a loadable bundle, drop it from
`default_registry()` and from `patches-modules`' (and any other
consumer's) compile-time dependencies. Vintage modules are only
available via runtime plugin load.

## Acceptance criteria

- [ ] `patches-modules/Cargo.toml` (and any other crate currently
      depending on `patches-vintage` for registration) no longer
      lists it as a dependency.
- [ ] `default_registry()` contains no vintage module entries.
- [ ] Workspace builds cleanly; no dead `use` or feature-flag relics.
- [ ] Any existing test that implicitly relied on vintage modules in
      the default registry is updated to load the bundle explicitly or
      is moved into integration tests under E095.

## Notes

Makes patches-vintage the forcing function test for the whole plugin
pipeline — if it works for vintage, it works for any third-party
bundle.
