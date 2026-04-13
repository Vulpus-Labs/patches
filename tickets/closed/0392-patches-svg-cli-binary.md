---
id: "0392"
title: patches-svg-cli binary
priority: medium
created: 2026-04-13
---

## Summary

New binary that reads a `.patches` file and writes an SVG rendering of the
expanded patch. Useful for scripting, CI, and generating images for the
mdBook manual.

Like the LSP (ticket 0390), rendering goes FlatPatch-direct: no
interpreter build, no module registry, no audio deps.

## Scope

- New crate `patches-svg-cli` (binary-only; `publish = false`).
- CLI shape:

  ```text
  patches-svg <input.patches> [-o <output.svg>]
              [--include-path DIR]... [--theme light|dark]
  ```

  - Stdout if `-o` not given.
  - Multiple `--include-path` accepted; forwarded to the DSL loader.
  - Non-zero exit on parse/expand error; diagnostics printed to stderr.
- Dependencies: `patches-dsl`, `patches-layout`, `patches-svg`, `clap`
  (argparser — **confirm before adding** as it is not currently a
  workspace dependency; fallback is a hand-rolled argv parser).
- Pipeline:
  1. Read input file + any `--include-path` directories via
     `patches_dsl::load_with`.
  2. `patches_dsl::expand` → `FlatPatch`.
  3. `patches_svg::render_svg(&flat, &opts)`.
  4. Write to `-o` or stdout.

## Acceptance criteria

- [ ] `cargo run -p patches-svg-cli -- path/to/patch.patches -o out.svg`
      produces a valid SVG on disk.
- [ ] Stdout mode works (`... patch.patches > out.svg`).
- [ ] Parse/expand errors exit non-zero with diagnostics on stderr.
- [ ] `--include-path` resolves includes from the given directories.
- [ ] Smoke test in `patches-integration-tests` or the crate itself runs
      the binary against a fixture patch.
- [ ] `cargo clippy` clean.

## Notes

- Ask before adding `clap` (per CLAUDE.md general conventions).
- Consider a later follow-up to wire this into `docs/` build so example
  patches in the manual auto-render to SVG.
