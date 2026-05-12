---
id: "0876"
title: Drop extracted modules from default_registry; wire stdlib bundles into PluginScanner
priority: medium
created: 2026-05-11
---

## Summary

After tickets 0873 + 0874 extract drums and fft modules into their
own bundle crates, the host must:

1. Stop registering them in-process via
   `patches-modules::default_registry()`.
2. Discover them at startup via `PluginScanner` reading a default
   search path that includes the three stdlib bundles (vintage,
   drums, fft).

This is the final cutover so the host runtime treats all three
stdlib bundles uniformly — same path vintage already follows per
ticket 0570 / ADR 0045 Spike 8 Phase C.

## Acceptance criteria

- [x] Drop drum module registrations (kick, snare, hihat, cymbal,
      tom, clap_drum, claves) from
      [patches-modules/src/lib.rs](../../patches-modules/src/lib.rs)
      `default_registry()` (done in 0873).
- [x] Drop pitch_shift + convolution_reverb registrations same
      (done in 0874).
- [x] Update comment near
      [patches-modules/src/lib.rs:189](../../patches-modules/src/lib.rs#L189)
      to list all three stdlib bundles loaded via PluginScanner.
- [x] `patches-modules` no longer depends on patches-drums or
      patches-fft-bundle. Vintage already not a dep.
- [x] PluginScanner has a documented default search path
      (`patches_ffi::stdlib_scanner`) keyed off `PATCHES_PLUGIN_PATH`
      with workspace `target/<profile>/` and `$EXE_DIR/plugins/`
      fallbacks.
- [x] Host startup logs the discovered bundle names + module counts
      ("stdlib bundles: N module(s) registered"). Surface a startup
      error if the path resolution yields zero search dirs OR if the
      scan loads zero modules, unless `--no-stdlib` is passed.
- [x] patches-integration-tests updated previously (in 0873 / 0874)
      so any test that needed drum / pitch_shift / conv_reverb in
      the registry loads via PluginScanner (`registry_with_bundles`)
      or pulls the bundle as an rlib dev-dep
      (`patches_fft_bundle::register`).
- [x] `cargo clippy --workspace --all-targets -- -D warnings` and
      `cargo run -p patches-forbidden-edges` green.

## Notes

Depends on: 0873, 0874, 0875.

PluginScanner default path strategy:

- **Development** (cargo run / cargo test): scan
  `$CARGO_MANIFEST_DIR/target/<profile>/` for `lib*.dylib`/`.so`/`.dll`.
- **Player binary release**: scan `$EXE_DIR/plugins/` (tarball ships
  the dylibs there).
- **CLAP plugin**: scan `<bundle>/Contents/Resources/plugins/` (macOS
  CLAP bundle convention).
- **Override**: `$PATCHES_PLUGIN_PATH` env var, colon-separated.

The deploy.sh / packaging step gets a follow-up to bundle the three
cdylibs into the right place. May warrant its own ticket if it grows
beyond a few lines; today vintage is already bundled, so the
extension is mechanical.

Startup check: emit a single warning if drum/fft bundles are loadable
but not signature-verified against the host ABI version
(`descriptor_hash` mismatch — already detected at load time per E145
notes, surface clearly to user).

## Implementation notes

- `patches_ffi::stdlib_scanner()` resolves search paths in priority
  order: `PATCHES_PLUGIN_PATH` env (colon-separated) →
  `$EXE_DIR/plugins/` → `$EXE_DIR/..` (covers `cargo test`'s
  `target/<profile>/deps` → `target/<profile>` walk-up) → `$EXE_DIR`
  → `$CWD/target/{debug,release}`. Non-existent candidates drop out
  so the resulting `PluginScanner.paths` only contains real dirs.
- The CLAP plugin and LSP do not (yet) call `stdlib_scanner`; that
  is a one-line wiring follow-up if the bundles are needed in those
  hosts. The integration tests already explicitly scan each bundle
  dylib via `registry_with_bundles`, so coverage of the scanner
  pipeline is unaffected.
- Default-registry drum/fft pop happened in 0873/0874; only the
  comment and the scanner wiring landed in this ticket.
