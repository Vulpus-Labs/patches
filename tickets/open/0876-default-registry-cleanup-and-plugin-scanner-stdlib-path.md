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

- [ ] Drop drum module registrations (kick, snare, hihat, cymbal,
      tom, clap_drum, claves) from
      [patches-modules/src/lib.rs](../../patches-modules/src/lib.rs)
      `default_registry()`.
- [ ] Drop pitch_shift + convolution_reverb registrations same.
- [ ] Update comment near
      [patches-modules/src/lib.rs:222](../../patches-modules/src/lib.rs#L222)
      to list all three stdlib bundles loaded via PluginScanner
      (vintage, drums, fft).
- [ ] `patches-modules` no longer depends on patches-drums or
      patches-fft-bundle. Vintage already not a dep — confirm.
- [ ] PluginScanner has a documented default search path that
      includes the build artefact location of the three stdlib
      bundles. Decision: development-mode default path = workspace
      `target/<profile>/`, with override via env var.
- [ ] Host startup logs the discovered bundle names + module counts
      ("loaded 3 stdlib bundles: vintage (12), drums (7), fft (2)").
      Surface a startup error if zero stdlib bundles found AND no
      explicit `--no-stdlib` flag passed.
- [ ] patches-integration-tests updated: any test that expected
      drum/pitch_shift/conv_reverb to be in `default_registry()`
      either loads via PluginScanner or is rewritten to load the
      cdylib explicitly.
- [ ] `just push` green.

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
