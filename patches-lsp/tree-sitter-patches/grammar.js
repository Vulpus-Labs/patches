/// <reference types="tree-sitter-cli/dsl" />

// Any change here MUST also update patches-dsl/src/grammar.pest AND add a
// corpus entry in patches-lsp/tests/syntax_corpus/*.corpus. The corpus
// driver enforces pest/tree-sitter parity.

module.exports = grammar({
  name: "patches",

  extras: ($) => [/\s/, $.comment],

  word: ($) => $.ident,

  // GLR conflicts. `step_valued`'s cv2 sugar (`:cv2[>cv2_end]`) and
  // `step_slide_open`'s `:cv2` tail (`value[:cv2]>`) both match the
  // same `value:cv2` prefix; the choice between them depends on what
  // follows. Let GLR explore both and resolve at the next token.
  conflicts: ($) => [
    [$.step_cv2, $.step_cv2_tail],
    // `/value` may stand alone or carry a trailing `>cv1_end` slide
    // target. GLR explores both and resolves at the next token.
    [$.step_step_to],
  ],

  rules: {
    // ─── File root ──────────────────────────────────────────────────────
    file: ($) =>
      seq(
        repeat(choice($.include_directive, $.template, $.pattern_block, $.section_def, $.song_block)),
        optional($.patch)
      ),

    // ─── Include directives ────────────────────────────────────────────
    include_directive: ($) =>
      seq("include", field("path", $.string_lit)),

    // ─── Comments ───────────────────────────────────────────────────────
    comment: (_) => token(seq("#", /[^\n]*/)),

    // ─── Lexical atoms ──────────────────────────────────────────────────
    ident: (_) => /[a-zA-Z_][a-zA-Z0-9_-]*/,
    nat: (_) => /\d+/,
    int_lit: (_) => token(seq(optional("-"), /\d+/)),
    float_lit: (_) => token(seq(optional("-"), /\d+/, ".", /\d*/)),
    bool_lit: (_) => choice("true", "false"),
    string_lit: (_) => token(seq('"', /[^"]*/, '"')),

    // Scale number inside arrow brackets
    scale_num: (_) => token(seq(optional("-"), /\d+/, optional(seq(".", /\d*/)))),

    // ─── Unit-suffixed numeric literals ─────────────────────────────────
    // khz/hz/db, plus `c` (cents, N/1200) and `s` (semis, N/12).
    // Note: pest carries a trailing `!(ASCII_ALPHANUMERIC | "_")` boundary
    // so e.g. `1.0sin` backtracks past the unit (yielding `1.0` + `sin`).
    // Tree-sitter's regex engine does not support `\b`, so we rely on the
    // lexer's longest-match rule instead. The accepted language is identical
    // in delimited contexts; pure-concatenation cases like `1.0sin` produce
    // a parse error in both grammars (only the localization differs).
    float_unit: (_) =>
      token(
        seq(
          optional("-"),
          /\d+/,
          optional(seq(".", /\d*/)),
          /[kK]?[hH][zZ]|[dD][bB]|[cCsS]/
        )
      ),

    // ─── Note name literals ─────────────────────────────────────────────
    // Pest's `note_lit` has a `!(alphanum | "_" | "-")` boundary so
    // `C4-tag` falls through to ident (ticket 0949). Tree-sitter's regex
    // engine has no lookahead, and removing the token precedence to lean
    // on longest-match breaks the equal-length `C4` case (ident's `word`
    // status wins the tie). The pre-existing comment on `float_unit`
    // already records the same accepted-language-in-delimited-contexts
    // caveat; we tolerate the divergence here too — pest is the
    // canonical accept/reject decision.
    note_lit: (_) =>
      token(prec(1, seq(/[A-Ga-g]/, optional(choice("#", /[bB]/)), optional("-"), /\d+/))),

    // ─── Parameter references ───────────────────────────────────────────
    // Allow "/" so group-param indexed keys like <level/0> are valid. Mirrors
    // the pest `param_ref_ident` charset; `.` is NOT permitted (canonical
    // pest grammar excludes it).
    param_ref_ident: (_) => /[a-zA-Z_][a-zA-Z0-9_/-]*/,
    param_ref: ($) => seq("<", $.param_ref_ident, ">"),

    // ─── Value hierarchy ────────────────────────────────────────────────
    scalar: ($) =>
      choice(
        $.float_unit,
        $.float_lit,
        $.int_lit,
        $.bool_lit,
        $.note_lit,
        $.string_lit,
        $.param_ref,
        $.ident
      ),

    // file("path") — a file reference, syntactically distinct from a plain string.
    file_ref: ($) => seq("file", "(", choice($.string_lit, $.param_ref), ")"),

    value: ($) => choice($.file_ref, $.scalar),

    // ─── Module call-arg block ──────────────────────────────────────────
    alias_list: ($) => seq("[", comma_list($.ident), "]"),

    named_arg: ($) =>
      seq(field("name", $.ident), ":", choice($.alias_list, $.scalar)),

    shorthand_arg: ($) => prec(2, $.param_ref),

    bare_arg: ($) => prec(1, choice($.alias_list, $.scalar)),

    call_arg: ($) => choice($.named_arg, $.shorthand_arg, $.bare_arg),

    call_block: ($) => seq("(", comma_list($.call_arg), ")"),

    // param_index: literal index [N], arity wildcard [*ident], or alias [ident]
    param_index_arity: ($) => seq("*", $.ident),
    param_index: ($) =>
      seq("[", choice($.param_index_arity, $.nat, $.ident), "]"),

    // at-block
    at_block_index: ($) => choice($.nat, $.ident),
    at_block_entry: ($) => seq($.ident, ":", $.value),
    at_block_body: ($) => seq("{", comma_list($.at_block_entry), "}"),
    at_block: ($) => seq("@", $.at_block_index, optional(":"), $.at_block_body),

    // param_entry
    param_entry: ($) =>
      choice(
        $.at_block,
        seq($.ident, optional($.param_index), ":", $.value)
      ),

    param_block: ($) =>
      seq(
        "{",
        repeat(seq(choice($.param_ref, $.param_entry), optional(","))),
        "}"
      ),

    // ─── Module declaration ─────────────────────────────────────────────
    // Optional `stereo` prefix (ADR 0070). Word-boundary handling comes
    // from `word: $.ident` (literal keywords don't match longer
    // identifiers like `stereo_in`). The keyword is wrapped in a named
    // `stereo_kw` rule so the LSP's tree-nav can field-lookup
    // `module_decl.stereo` without re-walking the keyword children.
    stereo_kw: (_) => "stereo",
    module_decl: ($) =>
      seq(
        optional(field("stereo", $.stereo_kw)),
        "module",
        field("name", $.ident),
        ":",
        field("type", $.ident),
        optional($.call_block),
        optional($.param_block)
      ),

    // ─── Port references ────────────────────────────────────────────────
    module_ident: ($) => choice("$", $.ident),

    port_index_arity: ($) => seq("*", $.ident),
    port_index: ($) =>
      seq(
        "[",
        choice($.port_index_arity, $.nat, $.param_ref, $.ident),
        "]"
      ),

    port_label: ($) => choice($.param_ref, $.ident),
    port_ref: ($) =>
      seq($.module_ident, ".", $.port_label, optional($.port_index)),

    // ─── Arrows ─────────────────────────────────────────────────────────
    // float_unit before scale_num so "1s" parses as unit, not "1" + leftover.
    // note_lit before scale_num so `C4` parses as note.
    scale_endpoint: ($) =>
      choice($.param_ref, $.float_unit, $.note_lit, $.scale_num),
    range_kind: (_) => choice("uni", "bi"),
    scale_range: ($) =>
      seq($.range_kind, "(", $.scale_endpoint, ",", $.scale_endpoint, ")"),
    scale_val: ($) => choice($.scale_range, $.scale_endpoint),

    forward_arrow: ($) =>
      choice(seq("-[", $.scale_val, "]->"), "->"),

    backward_arrow: ($) =>
      choice(seq("<-[", $.scale_val, "]-"), "<-"),

    arrow: ($) => choice($.forward_arrow, $.backward_arrow),

    // ─── Tap targets (ADR 0054) ─────────────────────────────────────────
    // Cable RHS form: `~taptype(name, k: v, ...)` and compound
    // `~a+b+c(name, ...)`. The canonical pest grammar leaves the component
    // set OPEN (any ident); validation enforces the closed set at parse-
    // adjacent stage (ticket 0711) so unknown components surface a
    // structural diagnostic with a "did you mean" hint rather than a raw
    // parse error.
    //
    // `tap_components` directly holds the component idents (no
    // `tap_component` wrapper — inlined in ticket 0950 to mirror pest).
    tap_components: ($) => seq($.ident, repeat(seq("+", $.ident))),
    tap_target: ($) =>
      seq("~", $.tap_components, "(", $.ident, ")"),

    // ─── Host controls (ADR 0057) ───────────────────────────────────────
    // Top-level declarations of DAW-facing parameters. Desugared to a
    // synthesised `~host_control` module instance by the expander.
    host_control_kind: (_) =>
      choice("knob", "slider", "toggle", "trigger"),
    // Ticket 0950: value side narrowed from `value` to `scalar` to
    // reject `file(...)` / other compound values at parse time
    // (matches pest).
    host_control_field: ($) =>
      seq(field("name", $.ident), ":", field("value", $.scalar)),
    host_control_block: ($) =>
      seq(
        field("kind", $.host_control_kind),
        field("name", $.ident),
        "{",
        repeat(seq($.host_control_field, optional(","))),
        "}"
      ),

    // `~name` reference to a host control on a cable endpoint.
    // The leading `~` mirrors tap targets (ADR 0054 §2). tap_target is
    // tried first in cable_endpoint so `~name(...)` parses as a tap; the
    // `~name` form (no parens) falls through to here.
    host_control_ref: ($) => seq("~", $.ident),

    // ─── Connections ────────────────────────────────────────────────────
    // Mirror pest: `cable_endpoint` is a visible wrapper. tap_target first
    // so `~name(...)` parses as a tap before host_control_ref's bare
    // `~name` form would otherwise consume the ident.
    cable_endpoint: ($) =>
      choice($.tap_target, $.host_control_ref, $.port_ref),
    connection: ($) =>
      seq(
        $.cable_endpoint,
        repeat(seq(",", $.cable_endpoint)),
        $.arrow,
        $.cable_endpoint,
        repeat(seq(",", $.cable_endpoint))
      ),

    // ─── Statements ─────────────────────────────────────────────────────
    statement: ($) =>
      choice($.module_decl, $.song_block, $.pattern_block, $.host_control_block, $.connection),

    // ─── Port declarations (inside templates) ───────────────────────────
    port_group_decl: ($) =>
      seq($.ident, optional(seq("[", $.ident, "]"))),

    // Pest exposes `comma_port_decls` as a wrapper rule and tightens the
    // keyword/colon adjacency (`"in:"` is a single literal). Mirror that.
    // `prec.right` resolves the LR conflict on the trailing optional comma
    // by preferring to continue the list when followed by another decl.
    comma_port_decls: ($) =>
      prec.right(seq(
        $.port_group_decl,
        repeat(seq(",", $.port_group_decl)),
        optional(","),
      )),
    in_decl: ($) => seq(token(seq("in", ":")), $.comma_port_decls),
    out_decl: ($) => seq(token(seq("out", ":")), $.comma_port_decls),
    port_decls: ($) => seq(optional($.in_decl), $.out_decl),

    // ─── Template parameter declarations ────────────────────────────────
    type_name: (_) => choice("float", "int", "bool", "pattern", "song", "str"),

    param_decl: ($) =>
      seq(
        $.ident,
        optional(seq("[", $.ident, "]")),
        ":",
        $.type_name,
        optional(seq("=", $.scalar))
      ),

    param_decls: ($) => seq("(", comma_list($.param_decl), ")"),

    // ─── Template ───────────────────────────────────────────────────────
    template: ($) =>
      seq(
        "template",
        field("name", $.ident),
        optional($.param_decls),
        "{",
        $.port_decls,
        repeat($.statement),
        "}"
      ),

    // ─── Pattern block ─────────────────────────────────────────────────
    //
    // pattern name {
    //     channel_label: step step step ...
    // }
    pattern_block: ($) =>
      seq(
        "pattern",
        field("name", $.ident),
        "{",
        repeat($.channel_row),
        "}"
      ),

    // Pest exposes channel_row_cont as a wrapper for "| step …" continuations
    // and uses zero-or-more cardinality for steps. Mirror that here.
    //
    // Ticket 0948 removed the `slide(n, A, B)` macro generator (and
    // the `step_or_generator` wrapper that distinguished it from
    // `step`); rows are now flat sequences of `step` cells.
    channel_row_cont: ($) => seq("|", repeat($.step)),
    channel_row: ($) =>
      seq(
        field("label", $.ident),
        ":",
        repeat($.step),
        repeat($.channel_row_cont)
      ),

    // ─── Step notation (ADR 0077 unified grammar) ──────────────────────
    // Eight cell shapes:
    //   .            rest
    //   _            tie / continuation (channel-stateful)
    //   value        triggered note (`value`, `value:cv2`, `value*N`)
    //   value>value  one-tick slide sugar form (ADR 0077 equivalence,
    //                  `value>value` ≡ `value> /value`)
    //   value>       open multi-tick slide
    //   /value       change cv1 without retrigger (`StepTo`)
    //   >_           explicit slide-flow / open-from-current
    //   >value       close an open slide within this tick
    //
    // The `~` step-tie token from epic E152 has been retired (ADR 0077
    // § "Token ambiguity"). `~tap(...)` / `~name` cable endpoints are
    // unaffected.
    //
    // Numeric primaries reuse the canonical `float_lit` / `int_lit` /
    // `float_unit` tokens (no `step_*` numeric duplicates — collapsed
    // in ticket 0950 to mirror pest).
    step_note: (_) =>
      token(prec(2, seq(/[A-Ga-g]/, optional(choice("#", /[bB]/)), optional("-"), /\d+/))),
    step_trigger: (_) => token(prec(3, "x")),
    step_rest: (_) => token(prec(3, ".")),
    step_tie: (_) => token(prec(3, "_")),
    step_tie_flow: (_) => token(prec(4, ">_")),

    // Slide target for cv1 (used inside `step_valued_slide` for the
    // `value>value` sugar form): ">value".
    step_slide_target: ($) =>
      seq(">", choice($.float_unit, $.step_note, $.float_lit, $.int_lit)),

    // cv2 part: ":value" with optional slide ">value".
    // `prec.right` mirrors `step_valued_*` — bias toward shifting an
    // adjacent `>value` slide_target into the cv2 cell rather than
    // reducing and parsing `>` as the start of a new cell.
    step_cv2: ($) =>
      prec.right(seq(":", choice($.float_unit, $.float_lit, $.int_lit), optional($.step_slide_target))),

    step_repeat: ($) => seq("*", $.nat),

    // A full valued step. Split into two mutually-exclusive
    // alternatives in ticket 0950 so the grammar itself rules out the
    // nonsense `value>value*N` combination (sugar slide plus repeat).
    // `step_valued` stays as a parent so the LSP retains a stable
    // anchor for cursor-context classification.
    //
    // `prec.right` on the slide alternative biases the GLR parser
    // toward eating an immediately following `>value` slide_target
    // rather than reducing and parsing `>` as the start of a new
    // step_slide_close cell. For the unsugared `value>` (no value
    // after `>`), step_slide_open wins via the explicit conflict
    // declared above.
    //
    // Pest's `step_valued_*` rules are compound-atomic (`${...}`) so
    // they forbid implicit whitespace between the cv1 primary and its
    // `:cv2` / `>target` / `*N` modifiers (ticket 0949). Tree-sitter
    // has no atomic concept; its `extras: [/\s/]` permits whitespace
    // between tokens. The corpus driver tolerates this divergence
    // (the `step_valued_whitespace` entry is `pest_error`); pest is
    // the canonical grammar for accept/reject decisions.
    step_valued_slide: ($) =>
      prec.right(seq(
        choice($.float_unit, $.step_note, $.step_trigger, $.float_lit, $.int_lit),
        $.step_slide_target,
        optional($.step_cv2),
      )),
    step_valued_note: ($) =>
      prec.right(seq(
        choice($.float_unit, $.step_note, $.step_trigger, $.float_lit, $.int_lit),
        optional($.step_cv2),
        optional($.step_repeat),
      )),
    step_valued: ($) => choice($.step_valued_slide, $.step_valued_note),

    // New cell shapes (ADR 0077; ticket 0948 adds the optional `:cv2`
    // tail on slide open / step-to / close-in-tick). step_slide_open
    // uses a token-merge trick: `value[:cv2]>` adjacent (no
    // whitespace) where the next non-whitespace character is not a
    // value-primary (so `value>value` falls through to `step_valued`).
    step_cv2_tail: ($) =>
      seq(":", choice($.float_unit, $.float_lit, $.int_lit)),
    step_step_to: ($) =>
      prec(2, seq(
        "/",
        choice($.float_unit, $.step_note, $.float_lit, $.int_lit),
        optional($.step_slide_target),
        optional($.step_cv2_tail),
      )),
    step_slide_close: ($) =>
      prec(1, seq(
        ">",
        choice($.float_unit, $.step_note, $.float_lit, $.int_lit),
        optional($.step_cv2_tail),
      )),
    step_slide_open: ($) =>
      prec(3, seq(
        choice($.float_unit, $.step_note, $.float_lit, $.int_lit),
        optional($.step_cv2_tail),
        ">",
      )),

    // Order matters: tie_flow before slide_close (both lead with `>`);
    // slide_open ahead of step_valued so `value>` is recognised before
    // step_valued tries to match it with a missing slide target.
    step: ($) =>
      choice(
        $.step_rest,
        $.step_tie_flow,
        $.step_step_to,
        $.step_slide_close,
        $.step_slide_open,
        $.step_tie,
        $.step_valued,
      ),

    // The `slide(n, A, B)` macro generator was removed in ticket
    // 0948; authors write the cells inline.

    // ─── Song block (ADR 0035) ───────────────────────────────────────────
    // song name(lane, ...) { section | pattern | play | @loop ... }
    //
    // The tree-sitter grammar intentionally loosens row-sequence structure
    // (newline-significance) — the canonical pest grammar is the source of
    // truth. Here rows reduce to a bag of cells and row-groups; LSP uses
    // the result for navigation only.
    song_block: ($) =>
      seq(
        "song",
        field("name", $.ident),
        $.song_lanes,
        "{",
        repeat($.song_item),
        "}"
      ),

    song_lanes: ($) => seq("(", sep1($.ident, ","), ")"),

    song_item: ($) =>
      choice($.section_def, $.pattern_block, $.play_stmt, $.loop_marker),

    section_def: ($) =>
      seq("section", field("name", $.ident), "{", repeat($.row_elem), "}"),

    // A loose row element: a cell, a parenthesised repeat group, or a
    // comma (acts as filler).
    row_elem: ($) => choice($.song_cell, $.repeat_group, ","),

    repeat_group: ($) =>
      seq("(", repeat($.row_elem), ")", "*", $.nat),

    song_cell: ($) => choice($.param_ref, $.ident, "_"),

    play_stmt: ($) => seq("play", $.play_body),

    play_body: ($) =>
      choice($.inline_block, $.named_inline, $.play_expr),

    inline_block: ($) => seq("{", repeat($.row_elem), "}"),

    named_inline: ($) =>
      seq(field("name", $.ident), "{", repeat($.row_elem), "}"),

    play_expr: ($) => sep1($.play_term, ","),

    play_term: ($) => seq($.play_atom, optional(seq("*", $.nat))),

    play_atom: ($) => choice(seq("(", $.play_expr, ")"), $.ident),

    // Pest enforces a trailing word boundary (`!alphanum`); tree-sitter's
    // regex has no `\b`, so we rely on lexer longest-match. Accepted
    // language is identical: in valid input `@loop` is always followed by
    // structural punctuation; in invalid input like `@loopy` both
    // grammars reject (only error localization differs).
    loop_marker: (_) => token(seq("@", "loop")),

    // ─── Patch ──────────────────────────────────────────────────────────
    patch: ($) => seq("patch", "{", repeat($.statement), "}"),
  },
});

// Helper: comma-separated list with optional trailing comma
function comma_list(rule) {
  return optional(seq(rule, repeat(seq(",", rule)), optional(",")));
}

// Helper: one-or-more separated by a separator, with optional trailing
function sep1(rule, separator) {
  return seq(rule, repeat(seq(separator, rule)), optional(separator));
}
