/// <reference types="tree-sitter-cli/dsl" />

module.exports = grammar({
  name: "patches",

  extras: ($) => [/\s/, $.comment],

  word: ($) => $.ident,

  rules: {
    // ─── File root ──────────────────────────────────────────────────────
    file: ($) =>
      seq(
        repeat(choice($.include_directive, $.template, $.pattern_block, $.song_block)),
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
    float_unit: (_) =>
      token(
        seq(
          optional("-"),
          /\d+/,
          optional(seq(".", /\d*/)),
          /[kK]?[hH][zZ]|[dD][bB]/
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

    array: ($) => seq("[", comma_list($.value), "]"),

    table_entry: ($) => seq($.ident, ":", $.value),
    table: ($) => seq("{", comma_list($.table_entry), "}"),

    value: ($) => choice($.file_ref, $.table, $.array, $.scalar),

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
    at_block: ($) => seq("@", $.at_block_index, optional(":"), $.table),

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
    scale_val: ($) => choice($.param_ref, $.scale_num),

    forward_arrow: ($) =>
      choice(seq("-[", $.scale_val, "]->"), "->"),

    backward_arrow: ($) =>
      choice(seq("<-[", $.scale_val, "]-"), "<-"),

    arrow: ($) => choice($.forward_arrow, $.backward_arrow),

    // ─── Connections ────────────────────────────────────────────────────
    connection: ($) => seq($.port_ref, $.arrow, $.port_ref, repeat(seq(",", $.port_ref))),

    // ─── Statements ─────────────────────────────────────────────────────
    statement: ($) => choice($.module_decl, $.song_block, $.pattern_block, $.connection),

    // ─── Port declarations (inside templates) ───────────────────────────
    port_group_decl: ($) =>
      seq($.ident, optional(seq("[", $.ident, "]"))),

    in_decl: ($) =>
      seq("in", ":", $.port_group_decl, repeat(seq(",", $.port_group_decl))),
    out_decl: ($) =>
      seq("out", ":", $.port_group_decl, repeat(seq(",", $.port_group_decl))),
    port_decls: ($) => seq($.in_decl, $.out_decl),

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
          seq(optional("-"), /\d+/, ".", /\d*/, optional(/[kK]?[hH][zZ]|[dD][bB]/)),
          // integer with optional unit
          seq(optional("-"), /\d+/, /[kK]?[hH][zZ]|[dD][bB]/),
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

    // ─── Song block ─────────────────────────────────────────────────────
    //
    // song name {
    //     | channel1 | channel2 |
    //     | pattern1 | pattern2 |
    //     | pattern3 | pattern4 |  @loop
    // }
    song_block: ($) =>
      seq(
        "song",
        field("name", $.ident),
        "{",
        repeat($.song_row),
        "}"
      ),

    song_row: ($) =>
      seq(
        repeat1(seq("|", $.song_cell)),
        "|",
        optional($.loop_annotation)
      ),

    song_cell: ($) => choice($.param_ref, $.ident, "_"),

    loop_annotation: (_) => token(seq("@", "loop")),

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
