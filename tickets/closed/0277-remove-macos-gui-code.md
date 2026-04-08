---
id: "0277"
title: Remove macOS-specific GUI code and objc2 dependencies
priority: medium
created: 2026-04-07
---

## Summary

Once the vizia GUI is fully functional on all platforms, remove the
macOS-specific AppKit implementation and its dependencies.

## Acceptance criteria

- [ ] `gui_mac.rs` is deleted.
- [ ] `objc2`, `objc2-app-kit`, and `objc2-foundation` are removed from
      `patches-clap/Cargo.toml`.
- [ ] No `#[cfg(target_os = "macos")]` GUI code remains in `extensions.rs` or
      `plugin.rs` (platform-specific concerns are handled inside
      vizia/baseview).
- [ ] `mod gui_mac` removed from `lib.rs`.
- [ ] `cargo clippy -p patches-clap` passes with no warnings.
- [ ] `cargo test` passes.

## Notes

- This is intentionally a separate ticket from T-0276 so that the old
  implementation can serve as a fallback during development. Once T-0276 is
  verified in a real DAW, this cleanup can proceed.
- Depends on T-0276.
