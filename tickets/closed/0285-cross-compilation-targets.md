---
id: "0285"
title: Install Rust cross-compilation targets and verify builds
priority: high
created: 2026-04-08
---

## Summary

Ensure `cargo build --release -p patches-lsp --target <triple>` succeeds for
each distribution target. The LSP crate links tree-sitter C code via `cc`, so
the C cross-compilation toolchain must also be available.

## Acceptance criteria

- [ ] `rustup target add aarch64-apple-darwin x86_64-apple-darwin x86_64-unknown-linux-gnu x86_64-pc-windows-msvc`
- [ ] `cargo build --release -p patches-lsp --target aarch64-apple-darwin` succeeds locally (native on Apple Silicon)
- [ ] `cargo build --release -p patches-lsp --target x86_64-apple-darwin` succeeds locally (Rosetta or cross)
- [ ] Document any linker or C toolchain setup needed for Linux/Windows cross builds (these will be done in CI)
- [ ] Note the resulting binary sizes for the release notes

## Notes

- `patches-lsp/build.rs` uses `cc` to compile `tree-sitter-patches/src/parser.c`.
  Cross-compiling this requires the target's C compiler (e.g. `x86_64-linux-gnu-gcc`
  for Linux, or handled automatically by Xcode for macOS cross).
- Linux cross from macOS is tricky; the CI approach (T-0286) builds natively on
  each OS runner, side-stepping the cross-compilation issue.
- Windows MSVC target needs the Windows SDK; defer to CI (T-0289).
