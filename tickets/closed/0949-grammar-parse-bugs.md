---
id: "0949"
title: Grammar parse bugs — `bool_lit` word-boundary, `step_valued` atomicity
priority: medium
created: 2026-05-20
epic: E154
---

## Summary

Two latent parse hazards in `patches-dsl/src/grammar.pest`:

1. `bool_lit` word-boundary is too narrow. Current form:

   ```pest
   bool_lit = @{ ("true" | "false") ~ !(ASCII_ALPHANUMERIC | "_") }
   ```

   The lookahead omits `-`, but `ident` permits `-`
   (`ident = @{ (ASCII_ALPHA | "_") ~ (ASCII_ALPHANUMERIC | "_" | "-")* }`).
   So `true-foo` matches `bool_lit` as `true`, leaving `-foo`
   dangling — silently parsing into something the author did not
   write. The other keyword rules (`stereo_kw`, `song_silence`,
   `loop_marker`, `host_control_kind`, `step_tie`, `step_tie_flow`)
   correctly include `"-"` in the lookahead. `step_trigger`,
   `float_unit`, and `note_lit` share the same too-narrow form
   and need the same fix.

2. `step_valued` is non-atomic. Current form:

   ```pest
   step_valued = { primary ~ step_slide_target? ~ step_cv2? ~ step_repeat? }
   ```

   Because the rule is `{...}` and not `${...}`, implicit
   `WHITESPACE` leaks between the cv1 primary and its `:cv2`,
   `>target`, or `*N` modifiers. `C4 : 0.5 * 3` parses today as
   a valid `step_valued`. Every other step cell (`step_slide_open`,
   `step_slide_close`, `step_step_to`, `step_tie`, `step_tie_flow`,
   `step_repeat`) is compound-atomic; `step_valued` is the
   outlier.

## Acceptance criteria

### Grammar fixes

- [ ] `bool_lit`, `step_trigger`, `float_unit`, `note_lit`:
      word-boundary lookahead extended to
      `!(ASCII_ALPHANUMERIC | "_" | "-")`.
- [ ] `step_valued` promoted to compound-atomic (`${ ... }`).

### Tree-sitter parity

- [ ] `patches-lsp/tree-sitter-patches/grammar.js` mirrored where
      necessary. Tree-sitter generated parser regenerated and
      committed (`src/grammar.json`, `src/node-types.json`,
      `src/parser.c`).

### Tests

- [ ] Negative fixtures under
      `patches-dsl/tests/fixtures/errors/`:
  - `bool_lit_hyphen_boundary.patches` — `true-foo` as a scalar
    fails to parse, with a span covering the full token.
  - `step_valued_whitespace.patches` — `note: C4 : 0.5 . . .`
    fails to parse with a diagnostic at the stray whitespace.
- [ ] Corresponding parser unit tests in
      `patches-dsl/tests/parser/` assert the failure modes.
- [ ] Corpus entry in `patches-lsp/tests/syntax_corpus/` covers
      the new boundary cases; parity driver green.

### Regression

- [ ] All existing `.patches` files in tree parse identically;
      no behaviour change for well-formed input.
- [ ] `just push` green.

## Notes

- The `bool_lit` hazard has not been hit in the corpus because
  no current `.patches` file has a `true-` or `false-` prefixed
  identifier, but the form is grammatically reachable and the
  silent dangling-token misparse is exactly the kind of latent
  bug to fix when it's free.
- The `step_valued` hazard is more aesthetic than dangerous —
  patterns are typically written tight — but the inconsistency
  with the other step rules is the kind of thing that bites a
  future author who copies `step_slide_open`'s `${...}` form and
  wonders why `step_valued` allows what they expect to be a
  parse error.
- The negative fixtures are the load-bearing artefact; they lock
  in the contract so a future grammar refactor cannot
  accidentally re-introduce either hazard.
