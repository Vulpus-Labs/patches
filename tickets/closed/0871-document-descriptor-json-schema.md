---
id: "0871"
title: Document the descriptor JSON schema as a normative reference
priority: medium
created: 2026-05-11
epic: E145
---

## Summary

The plugin descriptor crosses the FFI as JSON in two places:

- `module_template()` returns a `ModuleDescriptorTemplate` blob at
  load time.
- `prepare()` receives a per-instance `ModuleDescriptor` blob.

The schema is implicit, defined by the hand-rolled deserializer in
[patches-ffi-common/src/json/de.rs](patches-ffi-common/src/json/de.rs)
(376 lines) and serializer in
[patches-ffi-common/src/json/ser.rs](patches-ffi-common/src/json/ser.rs)
(166 lines). External implementers — including a hypothetical C++
SDK — have no normative reference; they must reverse-engineer from
the parser code.

This ticket produces a stand-alone schema doc that becomes the
authority. The Rust code stays the canonical implementation; the
doc is what external SDKs target.

## Acceptance criteria

- [ ] New page under [docs/src/](docs/src/) (suggested
      `docs/src/abi/descriptor-schema.md`) covering the full schema
      for `ModuleDescriptor` and `ModuleDescriptorTemplate`.
- [ ] Each `ParameterKind` variant documented with its JSON tag and
      payload fields:
  - `Float { range, default }`
  - `Int { range, default }`
  - `Bool { default }`
  - `Enum { variants, default }`
  - `File { … }` (structural-only)
  - `SongName` (structural-only)
- [ ] `PortDescriptor` schema: `name`, `index`, `kind`
      (mono/poly/stereo), `mono_layout`, `poly_layout`.
- [ ] `ParameterDescriptor` schema: `name`, `index`, `parameter_type`.
- [ ] Distinction between `realtime_params` and `structural_params`
      called out (which kinds are valid in which list — File and
      SongName are structural-only; the others are realtime-only,
      modulo whatever overlaps actually exist; verify against
      [patches-core/src/modules/module_descriptor.rs](patches-core/src/modules/module_descriptor.rs)).
- [ ] Statement of stability guarantees: which fields are
      additive-friendly vs. which trigger an ABI bump if changed.
      Cross-reference the `descriptor_hash` algorithm in
      [patches-core/src/param_layout/hash.rs](patches-core/src/param_layout/hash.rs)
      — note that range/default are intentionally excluded from the
      hash (hash.rs:99-107) so tuning them does not force a refusal-
      to-load.
- [ ] One worked example (a small two-port one-param module) showing
      template JSON, instance JSON, and the two side-by-side.
- [ ] Linked from the manual table of contents.
- [ ] Link from [patches-ffi-common/src/json/mod.rs](patches-ffi-common/src/json/mod.rs)
      module-level doc comment back to the manual page.
- [ ] `just push` clean (no broken links in the mdBook build).

## Notes

The JSON ser/de is hand-rolled with no `serde`/`serde_json` dependency
([patches-ffi-common/Cargo.toml](patches-ffi-common/Cargo.toml)). That
is a feature: the schema is yours, not serde's, and the doc can spec
exactly what the parser accepts (e.g. tag discrimination, key names,
case sensitivity) without hedging on serde behaviour.

If the schema doc surfaces inconsistencies between the serializer and
deserializer, fix them in this ticket — drift between the two would
otherwise be a future load failure that only shows up via
descriptor_hash mismatch.

Companion ticket 0872 covers the binary packing formats
(ParamFrame, PortFrame, structural blob, CableValue layout). Keep
those out of this doc; cross-reference instead.
