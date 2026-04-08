---
id: "0269"
title: Descriptor instantiation and body analysis (phases 3-4)
priority: high
created: 2026-04-07
---

## Summary

Implement the final two phases of the semantic analysis pipeline: descriptor
instantiation (resolve module descriptors via the registry) and body/connection
analysis (validate parameters and connections against descriptors).

## Acceptance criteria

- [ ] Phase 3: for each concrete module instance, map shape args to
      `ModuleShape` and call `Registry::describe(name, shape)`. Store the
      resulting `ModuleDescriptor` in the analysis model. For unknown types,
      emit a diagnostic and continue. For missing/invalid shape args, fall back
      to `ModuleShape::default()`.
- [ ] For template instances, the template's declared `in:`/`out:` ports serve
      as the "descriptor" for connection validation in the enclosing scope.
- [ ] Phase 4: for each connection in each scope (patch body or template body),
      validate that the source port exists in the source module's descriptor
      outputs and the destination port exists in the destination module's
      descriptor inputs. Emit diagnostics for unknown ports or invalid indices.
- [ ] For each parameter entry in a module declaration, validate the parameter
      name exists in the module's descriptor. Emit a diagnostic for unknown
      parameter names.
- [ ] The analysis produces a `SemanticModel` (or similar) containing: the
      declaration map, resolved descriptors per module instance, and accumulated
      diagnostics with spans.
- [ ] Tests cover: valid patch with known modules (zero diagnostics), unknown
      module type, unknown parameter name, unknown port name, invalid port
      index, template instance with valid port wiring.

## Notes

- Parameter type validation (checking that a float param receives a float
  value) is desirable but can be deferred if it adds significant complexity.
  Name-level validation is the priority.
- The `SemanticModel` is what the LSP server will query for completions,
  diagnostics, and hover.
- `Registry` is constructed via `patches_modules::default_registry()`.
- Epic: E049
