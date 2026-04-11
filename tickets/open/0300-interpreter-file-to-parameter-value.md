---
id: "0300"
title: "patches-interpreter: map file() AST nodes to ParameterValue::File"
priority: high
created: 2026-04-11
---

## Summary

Extend the interpreter's parameter conversion to handle `Value::File` AST
nodes, producing `ParameterValue::File` with the path resolved against the
patch file's base directory.

## Acceptance criteria

- [ ] `convert_value` handles `Value::File` + `ParameterKind::File` → `ParameterValue::File(resolved_path)`
- [ ] Relative paths are resolved against `base_dir` (as passed to `build_with_base_dir`)
- [ ] Absolute paths are passed through unchanged
- [ ] Type mismatch (e.g. `file("x")` for a `Float` parameter) produces an `InterpretError`
- [ ] Empty file path `file("")` produces an `InterpretError`
- [ ] Validation checks the file extension against `ParameterKind::File { extensions }` and produces an `InterpretError` on mismatch
- [ ] `cargo test -p patches-interpreter` passes with new tests covering the above
- [ ] `cargo clippy -p patches-interpreter` clean

## Notes

This replaces the current convention of resolving string parameters named
`"path"` with explicit `File`-typed parameter handling. The ad-hoc
path-resolution logic for `ParameterKind::String` params named `"path"`
added earlier can be removed once ConvolutionReverb migrates to the `File`
parameter kind.

Epic: E056
ADR: 0028
Depends: 0297, 0298
