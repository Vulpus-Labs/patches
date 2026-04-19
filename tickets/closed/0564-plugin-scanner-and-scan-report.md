---
id: "0564"
title: PluginScanner and ScanReport shared type
priority: high
created: 2026-04-19
epic: "E094"
---

## Summary

Consolidate plugin discovery into a single `PluginScanner` that every
Registry consumer uses. Replace ad-hoc `scan_plugins(dir)` callers with
a scanner over a list of paths that returns a structured `ScanReport`.

## Acceptance criteria

- [ ] New public types in `patches-ffi` (or a new `patches-plugins`
      crate if `patches-ffi` is getting fat):
      ```rust
      pub struct PluginScanner { pub paths: Vec<PathBuf> }
      pub struct LoadedModule  { pub name: String, pub version: u32, pub path: PathBuf }
      pub struct Replacement   { pub name: String, pub from: u32, pub to: u32 }
      pub enum  SkipReason     { LowerVersion { name: String, existing: u32, candidate: u32 }, AbiMismatch { expected: u32, found: u32, path: PathBuf }, DuplicateInBundle { name: String, path: PathBuf } }
      pub struct ScanReport    { pub loaded: Vec<LoadedModule>, pub replaced: Vec<Replacement>, pub skipped: Vec<SkipReason>, pub errors: Vec<(PathBuf, String)> }
      impl PluginScanner { pub fn scan(&self, registry: &mut Registry) -> ScanReport; }
      ```
- [ ] Scanner walks each path (directory or file); for directories it
      enumerates dylibs with platform-appropriate extension.
- [ ] For each bundle: ABI check → manifest → per-entry version compare
      against registry → insert or skip, record outcome in report.
- [ ] Existing `scan_plugins(&Path)` callers migrated; old function
      either removed or kept as a thin wrapper.
- [ ] Unit tests exercise: fresh load, version-upgrade, version-downgrade
      (skipped), ABI mismatch (skipped), unreadable file (error entry),
      duplicate module-name within one bundle (skipped).

## Notes

ADR 0044 §4. Subprocess isolation is out of scope for this ticket
(future work per ADR 0044 §6).
