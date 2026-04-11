---
id: "0298"
title: "patches-dsl: parse file(\"path\") syntax"
priority: high
created: 2026-04-11
---

## Summary

Add `file("path")` as a new value form in the DSL grammar so that file
references are syntactically distinct from plain strings.

## Acceptance criteria

- [ ] PEG grammar updated: `file("...")` is a valid parameter value
- [ ] AST gains a `Value::File(String)` variant (or `Scalar::File(String)`)
- [ ] `file("relative/path.wav")` parses successfully
- [ ] `file("/absolute/path.wav")` parses successfully
- [ ] Plain strings remain unchanged — `path: "foo.wav"` is still `Scalar::Str`
- [ ] Template parameter substitution works inside file(): `file(<ir_path>)` expands correctly
- [ ] Parser error on malformed file() (missing quotes, missing parens) produces a clear message
- [ ] `FlatPatch` carries `Value::File` through expansion unchanged
- [ ] `cargo test -p patches-dsl` passes
- [ ] `cargo clippy -p patches-dsl` clean

## Notes

The file() form is syntactically similar to a function call but is a value
literal — it does not invoke anything at parse time. The expander should
treat it as opaque, only performing template substitution on the inner
string if it contains a template reference.

Epic: E056
ADR: 0028
