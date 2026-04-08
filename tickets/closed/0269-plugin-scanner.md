---
id: "0269"
title: "Plugin scanner and registry integration"
priority: medium
created: 2026-04-07
---

## Summary

Implement the plugin scanner that discovers shared libraries in a directory,
loads them, and registers them in the module registry. This is the integration
point that makes dynamically-loaded modules available to the DSL and patch
builder.

## Acceptance criteria

- [ ] `scan_plugins(dir: &Path) -> Vec<Result<(String, DylibModuleBuilder), String>>` in `patches-ffi`:
  - Enumerates files matching the platform-specific extension (`.dylib` macOS, `.so` Linux, `.dll` Windows)
  - For each file, calls `load_plugin`; on success, calls `describe` with a default shape to extract the module name
  - Returns vec of (module_name, builder) pairs or per-file error strings
  - A broken plugin does not prevent other plugins from loading
- [ ] `register_plugins(dir: &Path, registry: &mut Registry) -> Vec<String>` convenience function:
  - Calls `scan_plugins`
  - Registers each successful builder via `Registry::register_builder`
  - Returns collected error messages
- [ ] Platform-specific extension detection works on macOS (primary dev platform)
- [ ] Integration test: place the Gain test plugin `.dylib` in a temp directory, call `register_plugins`, verify the module is discoverable and buildable via the registry's `create` method
- [ ] Integration test: place an invalid file (e.g. empty file) in the directory, verify it produces an error but does not prevent other plugins from loading
- [ ] `cargo clippy` clean

## Notes

- The caller (e.g. `patches-player`) decides the plugin directory path. A
  reasonable default is `plugins/` next to the patch file. This ticket does
  not modify `patches-player` — it provides the scanning API only.
- `scan_plugins` should log or collect errors rather than failing on the first
  bad file. The user needs to know which plugins failed and why.
- Symlinks in the plugin directory should be followed (a common pattern for
  development: symlink the build output into the plugins dir).

Epic: E052
ADR: 0025
Depends: 0265
