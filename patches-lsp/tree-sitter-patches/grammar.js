/// <reference types="tree-sitter-cli/dsl" />

module.exports = grammar({
  name: "patches",

  extras: ($) => [/\s/, $.comment],

  word: ($) => $.ident,

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
    note_lit: (_) =>
      token(prec(1, seq(/[A-Ga-g]/, optional(choice("#", /[bB]/)), optional("-"), /\d+/))),

    // ─── Parameter references ───────────────────────────────────────────
    param_ref_ident: (_) => /[a-zA-Z_][a-zA-Z0-9_/.-]*/,
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

    // ─── Shape and param blocks ─────────────────────────────────────────
    alias_list: ($) => seq("[", comma_list($.ident), "]"),

    shape_arg: ($) =>
      seq(field("name", $.ident), ":", choice($.alias_list, $.scalar)),

    shape_block: ($) => seq("(", comma_list($.shape_arg), ")"),

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
    module_decl: ($) =>
      seq(
        "module",
        field("name", $.ident),
        ":",
        field("type", $.ident),
        optional($.shape_block),
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
    scale_val: ($) => choice($.param_ref, $.float_unit, $.scale_num),

    forward_arrow: ($) =>
      choice(seq("-[", $.scale_val, "]->"), "->"),

    backward_arrow: ($) =>
      choice(seq("<-[", $.scale_val, "]-"), "<-"),

    arrow: ($) => choice($.forward_arrow, $.backward_arrow),

    // ─── Tap targets (ADR 0054) ─────────────────────────────────────────
    // Cable RHS form: `~taptype(name, k: v, ...)` and compound
    // `~a+b+c(name, ...)`. The component set is closed; non-listed
    // components produce an ERROR node localised to the tap target.
    tap_type: (_) =>
      choice("meter", "osc", "spectrum", "gate_led", "trigger_led"),
    tap_components: ($) => seq($.tap_type, repeat(seq("+", $.tap_type))),
    tap_qualifier: ($) => $.ident,
    tap_param_key: ($) =>
      choice(seq($.tap_qualifier, ".", $.ident), $.ident),
    tap_param: ($) => seq($.tap_param_key, ":", $.value),
    tap_params: ($) =>
      seq($.tap_param, repeat(seq(",", $.tap_param)), optional(",")),
    tap_name: ($) => $.ident,
    tap_target: ($) =>
      seq(
        "~",
        $.tap_components,
        "(",
        $.tap_name,
        optional(seq(",", $.tap_params)),
        ")"
      ),

    // ─── Connections ────────────────────────────────────────────────────
    // _cable_endpoint is a hidden alias (leading underscore) so the
    // existing port_ref-under-connection tree shape is preserved when no
    // tap target is involved. With a tap target on either side, the
    // tap_target node appears directly under connection.
    _cable_endpoint: ($) => choice($.tap_target, $.port_ref),
    connection: ($) =>
      seq(
        $._cable_endpoint,
        $.arrow,
        $._cable_endpoint,
        repeat(seq(",", $._cable_endpoint))
      ),

    // ─── Statements ─────────────────────────────────────────────────────
    statement: ($) => choice($.module_decl, $.song_block, $.pattern_block, $.connection),

    // ─── Port declarations (inside templates) ───────────────────────────
    port_group_decl: ($) =>
      seq($.ident, optional(seq("[", $.ident, "]"))),

    in_decl: ($) =>
      seq("in", ":", $.port_group_decl, repeat(seq(",", $.port_group_decl))),
    out_decl: ($) =>
      seq("out", ":", $.port_group_decl, repeat(seq(",", $.port_group_decl))),
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

    channel_row: ($) =>
      seq(
        field("label", $.ident),
        ":",
        repeat1($.step),
        repeat(seq("|", repeat1($.step)))
      ),

    // ─── Step notation ──────────────────────────────────────────────────
    step_cv2: ($) => seq(":", $.step_value, optional($.step_slide_target)),

    step_slide_target: ($) => seq(">", $.step_value),

    step_repeat: ($) => seq("*", $.nat),

    step_value: (_) =>
      token(
        prec(2, choice(
          // float with optional unit
          seq(optional("-"), /\d+/, ".", /\d*/, optional(/[kK]?[hH][zZ]|[dD][bB]|[cCsS]/)),
          // integer with optional unit
          seq(optional("-"), /\d+/, /[kK]?[hH][zZ]|[dD][bB]|[cCsS]/),
          // plain integer
          seq(optional("-"), /\d+/),
          // note literal
          seq(/[A-Ga-g]/, optional(choice("#", /[bB]/)), optional("-"), /\d+/)
        ))
      ),

    step: ($) =>
      choice(
        $.step_rest,
        $.step_tie,
        $.step_trigger,
        $.slide_generator,
        $.step_active
      ),

    step_rest: (_) => token(prec(3, ".")),
    step_tie: (_) => token(prec(3, "~")),
    step_trigger: ($) =>
      prec(2, seq(
        token(prec(3, "x")),
        optional($.step_cv2),
        optional($.step_repeat)
      )),

    step_active: ($) =>
      prec(1, seq(
        $.step_value,
        optional($.step_slide_target),
        optional($.step_cv2),
        optional($.step_repeat)
      )),

    // slide(count, start, end)
    slide_generator: ($) =>
      seq("slide", "(", $.step_value, ",", $.step_value, ",", $.step_value, ")"),

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
