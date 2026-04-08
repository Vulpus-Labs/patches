---
id: "0138"
title: DSL example patch corpus
priority: high
created: 2026-03-19
---

## Summary

Before the PEG grammar is written, author a corpus of hand-written `.patches`
files that exercise every construct in the syntax specified by ADR 0006. These
files serve simultaneously as:

- A human-readable specification of what valid patches look like.
- Positive-case fixtures for the parser tests in T-0139.
- Long-term living examples of the DSL for new contributors.

A companion set of negative-case files, each containing exactly one syntax
error, anchors the grammar's rejection behaviour.

## Acceptance criteria

- [x] `patches-dsl/tests/fixtures/` contains the following positive-case files:

  - `simple.patches` — flat patch, no templates. One or two modules, a few
    connections using both `->` and `<-`. No scaling, no indexed ports, no
    shape args. Baseline smoke-test for the parser.

  - `scaled_and_indexed.patches` — flat patch exercising `-[N]->` and
    `<-[N]-` (including a negative scale), and at least one indexed port
    reference `module.port[k]`.

  - `array_params.patches` — flat patch with a module that takes an array
    param (sequencer-style) and a module that takes a table-valued array
    param. Exercises multi-line param blocks.

  - `voice_template.patches` — defines one template with declared params and
    default values. Instantiates the template twice in the patch body, once
    with all defaults and once with param overrides. Uses `<-` for template
    in-port bindings, `<-` for out-port bindings, `->` for internal
    connections. Includes at least one scaled template port (`<-[N]-` and
    `-[N]->`).

  - `nested_templates.patches` — defines two templates, the second
    instantiating the first in its body. Instantiates the outer template from
    the patch block. Proves the grammar handles nesting at the syntactic level.

- [x] `patches-dsl/tests/fixtures/errors/` contains the following
  negative-case files, each with exactly one syntax error and a comment on
  line 1 naming the error:

  - `missing_arrow.patches` — two port refs on a line with no arrow between
    them.
  - `malformed_index.patches` — port index using a non-integer (`module.in[x]`).
  - `malformed_scale.patches` — scale bracket not closed (`-[0.5-> dest`).
  - `unknown_arrow.patches` — an `=>` arrow (not a valid connection operator).
  - `bare_module.patches` — `module` keyword with no name or type.
  - `unclosed_param_block.patches` — param block `{` with no closing `}`.

- [x] Every positive fixture is syntactically valid according to ADR 0006 and
  covers the constructs described above; reviewable without running the parser.

- [x] Every negative fixture contains exactly the syntax error named in its
  leading comment, and no other errors.

## Notes

The fixtures do not need to be semantically valid patches (real module type
names, real port names). The grammar operates on structure only; semantic
validation is the responsibility of `patches-interpreter` (T-0143 onwards).
Using realistic but possibly invented names (e.g. `Osc`, `Adsr`, `freq`,
`gate`) makes the files easier to read.

See ADR 0006 for the full grammar sketch and worked examples of each syntax
construct.
