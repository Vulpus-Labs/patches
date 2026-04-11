---
id: "0306"
title: "Integration tests: file parameter round-trip"
priority: medium
created: 2026-04-11
---

## Summary

Add integration tests that exercise the full file parameter pipeline: DSL
parsing → interpreter → planner (with `process_file`) → plan adoption →
module receives `FloatBuffer`.

## Acceptance criteria

- [ ] Test: a `.patches` source with `file("path/to/ir.wav")` parses, interprets, and builds a plan successfully when the file exists
- [ ] Test: a `.patches` source with `file("nonexistent.wav")` fails at plan build time with a descriptive error message
- [ ] Test: relative file paths resolve correctly against a known base directory
- [ ] Test: ConvolutionReverb with a real IR file produces non-silent output after plan adoption
- [ ] Test: file extension validation rejects unsupported extensions (e.g. `file("data.txt")` for an IR parameter)
- [ ] Test: hot-reload with a changed file parameter triggers re-processing and the module receives updated data
- [ ] Tests live in `patches-integration-tests`
- [ ] `cargo test -p patches-integration-tests` passes
- [ ] `cargo clippy -p patches-integration-tests` clean

## Notes

A small test WAV or AIFF file should be committed to
`patches-integration-tests/fixtures/` (or similar) for these tests. Keep it
short (a few hundred samples) to avoid bloating the repository.

Epic: E056
ADR: 0028
Depends: 0301, 0303
