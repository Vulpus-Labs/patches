---
id: "0299"
title: "patches-core: FileProcessor trait and registry support"
priority: high
created: 2026-04-11
---

## Summary

Define the `FileProcessor` trait and extend the registry so that the
planner can look up and call `process_file` for modules that accept file
parameters.

## Acceptance criteria

- [ ] `FileProcessor` trait defined in patches-core with signature:
      `fn process_file(env: &AudioEnvironment, shape: &ModuleShape, param_name: &str, path: &str) -> Result<Vec<f32>, String> where Self: Sized`
- [ ] `ModuleBuilder` trait (or a parallel mechanism) can report whether a module supports `FileProcessor`
- [ ] Registry exposes `process_file(module_name, env, shape, param_name, path) -> Result<Vec<f32>, String>` that delegates to the registered builder
- [ ] Registry returns a clear error if `process_file` is called for a module that does not implement `FileProcessor`
- [ ] The registration path for `FileProcessor` works with the existing `register::<M>()` pattern — modules that implement both `Module` and `FileProcessor` are automatically registered for both
- [ ] `cargo test -p patches-core` passes
- [ ] `cargo clippy -p patches-core` clean

## Notes

`process_file` is a static method — it does not require a module instance.
The registry must store a function pointer (or closure) alongside the
module builder. This is analogous to how `ModuleBuilder` stores `describe`
and `build` — another static capability of the module type.

Epic: E056
ADR: 0028
Depends: 0297
