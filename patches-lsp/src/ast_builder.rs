//! Tree-sitter CST to tolerant AST builder.
//!
//! Walks the tree-sitter CST and produces the tolerant AST, accumulating
//! diagnostics for ERROR and MISSING nodes encountered during the walk.

use tree_sitter::{Node, Tree};

use crate::ast::{
    Arrow, AtBlockIndex, Connection, Direction, File, Ident, ModuleDecl, ParamDecl,
    ParamEntry, ParamIndex, ParamType, Patch, PortGroupDecl, PortIndex, PortLabel, PortRef, Scalar,
    ShapeArg, ShapeArgValue, Span, Statement, Template, Value,
};

/// Classification of a diagnostic for severity mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiagnosticKind {
    SyntaxError,
    MissingToken,
    UnknownModuleType,
    DependencyCycle,
    UnknownPort,
    UnknownParameter,
    InvalidValue,
}

impl DiagnosticKind {
    /// Map diagnostic kind to LSP severity.
    pub fn severity(self) -> tower_lsp::lsp_types::DiagnosticSeverity {
        use tower_lsp::lsp_types::DiagnosticSeverity;
        match self {
            DiagnosticKind::SyntaxError
            | DiagnosticKind::MissingToken
            | DiagnosticKind::UnknownModuleType
            | DiagnosticKind::DependencyCycle => DiagnosticSeverity::ERROR,
            DiagnosticKind::UnknownPort
            | DiagnosticKind::UnknownParameter
            | DiagnosticKind::InvalidValue => DiagnosticSeverity::WARNING,
        }
    }
}

/// A diagnostic emitted during AST construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Diagnostic {
    pub span: Span,
    pub message: String,
    pub kind: DiagnosticKind,
}

/// Frequency of C0 in Hz (A4 = 440 Hz; C0 is 57 semitones below A4).
const C0_HZ: f64 = 16.351_597_831_287_414;

fn span_of(node: Node) -> Span {
    Span::new(node.start_byte(), node.end_byte())
}

fn node_text<'a>(node: Node, source: &'a str) -> &'a str {
    &source[node.start_byte()..node.end_byte()]
}

/// Build a tolerant AST from a tree-sitter parse tree.
pub(crate) fn build_ast(tree: &Tree, source: &str) -> (File, Vec<Diagnostic>) {
    let mut diags = Vec::new();
    let root = tree.root_node();
    let file = build_file(root, source, &mut diags);
    (file, diags)
}

fn collect_errors(node: Node, diags: &mut Vec<Diagnostic>) {
    if node.is_error() {
        diags.push(Diagnostic {
            span: span_of(node),
            message: "syntax error".to_string(),
            kind: DiagnosticKind::SyntaxError,
        });
    } else if node.is_missing() {
        diags.push(Diagnostic {
            span: span_of(node),
            message: format!("missing {}", node.kind()),
            kind: DiagnosticKind::MissingToken,
        });
    }
}

fn walk_errors(node: Node, diags: &mut Vec<Diagnostic>) {
    collect_errors(node, diags);
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.is_error() || child.is_missing() {
            collect_errors(child, diags);
        }
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn named_children_of_kind<'a>(
    node: Node<'a>,
    kind: &str,
) -> Vec<Node<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|c| c.is_named() && c.kind() == kind)
        .collect()
}

fn first_named_child_of_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let result = node.children(&mut cursor)
        .find(|c| c.is_named() && c.kind() == kind);
    result
}

fn build_ident(node: Node, source: &str) -> Ident {
    Ident {
        name: node_text(node, source).to_string(),
        span: span_of(node),
    }
}

// ─── File ───────────────────────────────────────────────────────────────────

fn build_file(node: Node, source: &str, diags: &mut Vec<Diagnostic>) -> File {
    walk_errors(node, diags);

    let mut templates = Vec::new();
    let mut patch = None;

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "template" => templates.push(build_template(child, source, diags)),
            "patch" => patch = Some(build_patch(child, source, diags)),
            _ => {}
        }
    }

    File {
        templates,
        patch,
        span: span_of(node),
    }
}

// ─── Patch ──────────────────────────────────────────────────────────────────

fn build_patch(node: Node, source: &str, diags: &mut Vec<Diagnostic>) -> Patch {
    walk_errors(node, diags);

    let body = named_children_of_kind(node, "statement")
        .into_iter()
        .filter_map(|s| build_statement(s, source, diags))
        .collect();

    Patch {
        body,
        span: span_of(node),
    }
}

// ─── Statement ──────────────────────────────────────────────────────────────

fn build_statement(node: Node, source: &str, diags: &mut Vec<Diagnostic>) -> Option<Statement> {
    walk_errors(node, diags);

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if !child.is_named() {
            continue;
        }
        match child.kind() {
            "module_decl" => return Some(Statement::Module(build_module_decl(child, source, diags))),
            "connection" => return Some(Statement::Connection(Box::new(build_connection(child, source, diags)))),
            _ => {}
        }
    }
    None
}

// ─── Module declaration ─────────────────────────────────────────────────────

fn build_module_decl(node: Node, source: &str, diags: &mut Vec<Diagnostic>) -> ModuleDecl {
    walk_errors(node, diags);

    let name = node.child_by_field_name("name").map(|n| build_ident(n, source));
    let type_name = node.child_by_field_name("type").map(|n| build_ident(n, source));

    let shape = first_named_child_of_kind(node, "shape_block")
        .map(|sb| build_shape_block(sb, source, diags))
        .unwrap_or_default();

    let params = first_named_child_of_kind(node, "param_block")
        .map(|pb| build_param_block(pb, source, diags))
        .unwrap_or_default();

    ModuleDecl {
        name,
        type_name,
        shape,
        params,
        span: span_of(node),
    }
}

// ─── Shape block ────────────────────────────────────────────────────────────

fn build_shape_block(node: Node, source: &str, diags: &mut Vec<Diagnostic>) -> Vec<ShapeArg> {
    walk_errors(node, diags);
    named_children_of_kind(node, "shape_arg")
        .into_iter()
        .map(|n| build_shape_arg(n, source, diags))
        .collect()
}

fn build_shape_arg(node: Node, source: &str, diags: &mut Vec<Diagnostic>) -> ShapeArg {
    walk_errors(node, diags);

    let name = node.child_by_field_name("name").map(|n| build_ident(n, source));

    let value = if let Some(al) = first_named_child_of_kind(node, "alias_list") {
        Some(build_alias_list(al, source, diags))
    } else {
        first_named_child_of_kind(node, "scalar")
            .map(|s| ShapeArgValue::Scalar(build_scalar(s, source, diags)))
    };

    ShapeArg {
        name,
        value,
        span: span_of(node),
    }
}

fn build_alias_list(node: Node, source: &str, diags: &mut Vec<Diagnostic>) -> ShapeArgValue {
    walk_errors(node, diags);
    let idents = named_children_of_kind(node, "ident")
        .into_iter()
        .map(|n| build_ident(n, source))
        .collect();
    ShapeArgValue::AliasList(idents)
}

// ─── Param block ────────────────────────────────────────────────────────────

fn build_param_block(node: Node, source: &str, diags: &mut Vec<Diagnostic>) -> Vec<ParamEntry> {
    walk_errors(node, diags);

    let mut entries = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if !child.is_named() {
            continue;
        }
        match child.kind() {
            "param_entry" => entries.push(build_param_entry(child, source, diags)),
            "param_ref" => {
                if let Some(pri) = first_named_child_of_kind(child, "param_ref_ident") {
                    entries.push(ParamEntry::Shorthand(build_ident(pri, source)));
                }
            }
            _ => {}
        }
    }
    entries
}

fn build_param_entry(node: Node, source: &str, diags: &mut Vec<Diagnostic>) -> ParamEntry {
    walk_errors(node, diags);

    // Check if this is an at-block
    if let Some(ab) = first_named_child_of_kind(node, "at_block") {
        return build_at_block(ab, source, diags);
    }

    // Otherwise it's a key-value entry: ident [param_index] : value
    let name = first_named_child_of_kind(node, "ident").map(|n| build_ident(n, source));
    let index = first_named_child_of_kind(node, "param_index")
        .map(|n| build_param_index(n, source, diags));
    let value = first_named_child_of_kind(node, "value")
        .map(|n| build_value(n, source, diags));

    ParamEntry::KeyValue {
        name,
        index,
        value,
        span: span_of(node),
    }
}

fn build_param_index(node: Node, source: &str, diags: &mut Vec<Diagnostic>) -> ParamIndex {
    walk_errors(node, diags);

    if let Some(arity) = first_named_child_of_kind(node, "param_index_arity") {
        if let Some(id) = first_named_child_of_kind(arity, "ident") {
            return ParamIndex::Arity(node_text(id, source).to_string());
        }
    }
    if let Some(nat) = first_named_child_of_kind(node, "nat") {
        if let Ok(n) = node_text(nat, source).parse::<u32>() {
            return ParamIndex::Literal(n);
        }
    }
    if let Some(id) = first_named_child_of_kind(node, "ident") {
        return ParamIndex::Alias(node_text(id, source).to_string());
    }

    // Fallback — shouldn't normally happen
    ParamIndex::Literal(0)
}

fn build_at_block(node: Node, source: &str, diags: &mut Vec<Diagnostic>) -> ParamEntry {
    walk_errors(node, diags);

    let index = first_named_child_of_kind(node, "at_block_index")
        .map(|n| build_at_block_index(n, source, diags));

    let entries = first_named_child_of_kind(node, "table")
        .map(|t| build_table_entries(t, source, diags))
        .unwrap_or_default();

    ParamEntry::AtBlock {
        index,
        entries,
        span: span_of(node),
    }
}

fn build_at_block_index(node: Node, source: &str, diags: &mut Vec<Diagnostic>) -> AtBlockIndex {
    walk_errors(node, diags);

    if let Some(nat) = first_named_child_of_kind(node, "nat") {
        if let Ok(n) = node_text(nat, source).parse::<u32>() {
            return AtBlockIndex::Literal(n);
        }
    }
    if let Some(id) = first_named_child_of_kind(node, "ident") {
        return AtBlockIndex::Alias(node_text(id, source).to_string());
    }

    AtBlockIndex::Literal(0)
}

// ─── Values ─────────────────────────────────────────────────────────────────

fn build_value(node: Node, source: &str, diags: &mut Vec<Diagnostic>) -> Value {
    walk_errors(node, diags);

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if !child.is_named() {
            continue;
        }
        match child.kind() {
            "scalar" => return Value::Scalar(build_scalar(child, source, diags)),
            "array" => return build_array(child, source, diags),
            "table" => {
                let entries = build_table_entries(child, source, diags);
                return Value::Table(entries);
            }
            _ => {}
        }
    }
    // Fallback for incomplete parse
    Value::Scalar(Scalar::Int(0))
}

fn build_array(node: Node, source: &str, diags: &mut Vec<Diagnostic>) -> Value {
    walk_errors(node, diags);
    let items = named_children_of_kind(node, "value")
        .into_iter()
        .map(|n| build_value(n, source, diags))
        .collect();
    Value::Array(items)
}

fn build_table_entries(
    node: Node,
    source: &str,
    diags: &mut Vec<Diagnostic>,
) -> Vec<(Ident, Value)> {
    walk_errors(node, diags);
    named_children_of_kind(node, "table_entry")
        .into_iter()
        .filter_map(|te| {
            let key = first_named_child_of_kind(te, "ident").map(|n| build_ident(n, source))?;
            let val = first_named_child_of_kind(te, "value")
                .map(|n| build_value(n, source, diags))
                .unwrap_or(Value::Scalar(Scalar::Int(0)));
            Some((key, val))
        })
        .collect()
}

fn build_scalar(node: Node, source: &str, diags: &mut Vec<Diagnostic>) -> Scalar {
    walk_errors(node, diags);

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if !child.is_named() {
            continue;
        }
        let text = node_text(child, source);
        match child.kind() {
            "int_lit" => {
                return Scalar::Int(text.parse().unwrap_or(0));
            }
            "float_lit" => {
                return Scalar::Float(text.parse().unwrap_or(0.0));
            }
            "bool_lit" => {
                return Scalar::Bool(text == "true");
            }
            "string_lit" => {
                // Strip surrounding quotes
                let inner = if text.len() >= 2 { &text[1..text.len() - 1] } else { text };
                return Scalar::Str(inner.to_string());
            }
            "note_lit" => {
                return Scalar::Float(parse_note_voct(text));
            }
            "float_unit" => {
                return Scalar::Float(parse_float_unit(text, diags, span_of(child)));
            }
            "param_ref" => {
                if let Some(pri) = first_named_child_of_kind(child, "param_ref_ident") {
                    return Scalar::ParamRef(build_ident(pri, source));
                }
                return Scalar::ParamRef(Ident { name: String::new(), span: span_of(child) });
            }
            "ident" => {
                // tree-sitter's `word` rule can cause note literals (C4, A#-1)
                // to be tokenised as idents. Detect the note pattern and convert.
                if looks_like_note(text) {
                    return Scalar::Float(parse_note_voct(text));
                }
                return Scalar::Ident(text.to_string());
            }
            _ => {}
        }
    }

    Scalar::Int(0)
}

// ─── Numeric conversions ────────────────────────────────────────────────────

/// Check if a string looks like a note literal (e.g. C4, A#-1, Bb2).
fn looks_like_note(s: &str) -> bool {
    let b = s.as_bytes();
    if b.is_empty() {
        return false;
    }
    if !matches!(b[0].to_ascii_lowercase(), b'a'..=b'g') {
        return false;
    }
    let mut pos = 1;
    if pos < b.len() && (b[pos] == b'#' || b[pos].eq_ignore_ascii_case(&b'b')) {
        pos += 1;
    }
    if pos >= b.len() {
        return false;
    }
    // Remaining must be an optional '-' followed by digits
    if b[pos] == b'-' {
        pos += 1;
    }
    if pos >= b.len() {
        return false;
    }
    b[pos..].iter().all(|c| c.is_ascii_digit())
}

fn note_class_semitone(letter: u8) -> i32 {
    match letter.to_ascii_lowercase() {
        b'c' => 0,
        b'd' => 2,
        b'e' => 4,
        b'f' => 5,
        b'g' => 7,
        b'a' => 9,
        b'b' => 11,
        _ => 0,
    }
}

fn parse_note_voct(s: &str) -> f64 {
    let b = s.as_bytes();
    if b.is_empty() {
        return 0.0;
    }
    let class = note_class_semitone(b[0]);
    let mut pos = 1usize;

    let accidental = if pos < b.len() && (b[pos] == b'#' || b[pos].eq_ignore_ascii_case(&b'b')) {
        let acc = if b[pos] == b'#' { 1i32 } else { -1i32 };
        pos += 1;
        acc
    } else {
        0i32
    };

    let octave: i32 = s[pos..].parse().unwrap_or(0);
    (octave * 12 + class + accidental) as f64 / 12.0
}

fn parse_float_unit(s: &str, diags: &mut Vec<Diagnostic>, span: Span) -> f64 {
    // Find where the unit suffix starts
    let lower = s.to_ascii_lowercase();
    let (num_str, unit) = if lower.ends_with("khz") {
        (&s[..s.len() - 3], "khz")
    } else if lower.ends_with("hz") {
        (&s[..s.len() - 2], "hz")
    } else if lower.ends_with("db") {
        (&s[..s.len() - 2], "db")
    } else {
        (s, "")
    };

    let num: f64 = num_str.parse().unwrap_or(0.0);

    match unit {
        "db" => 10.0_f64.powf(num / 20.0),
        "hz" => {
            if num <= 0.0 {
                diags.push(Diagnostic {
                    span,
                    message: format!("Hz value must be positive, got {num}"),
                    kind: DiagnosticKind::InvalidValue,
                });
                0.0
            } else {
                (num / C0_HZ).log2()
            }
        }
        "khz" => {
            let hz = num * 1000.0;
            if hz <= 0.0 {
                diags.push(Diagnostic {
                    span,
                    message: format!("kHz value must be positive, got {num}"),
                    kind: DiagnosticKind::InvalidValue,
                });
                0.0
            } else {
                (hz / C0_HZ).log2()
            }
        }
        _ => num,
    }
}

// ─── Connections ────────────────────────────────────────────────────────────

fn build_connection(node: Node, source: &str, diags: &mut Vec<Diagnostic>) -> Connection {
    walk_errors(node, diags);

    let port_refs = named_children_of_kind(node, "port_ref");
    let lhs = port_refs.first().map(|n| build_port_ref(*n, source, diags));
    let rhs = port_refs.get(1).map(|n| build_port_ref(*n, source, diags));
    let arrow = first_named_child_of_kind(node, "arrow")
        .map(|n| build_arrow(n, source, diags));

    Connection {
        lhs,
        arrow,
        rhs,
        span: span_of(node),
    }
}

fn build_port_ref(node: Node, source: &str, diags: &mut Vec<Diagnostic>) -> PortRef {
    walk_errors(node, diags);

    let module = first_named_child_of_kind(node, "module_ident").map(|mi| {
        // module_ident is either "$" or contains an ident child
        if let Some(id) = first_named_child_of_kind(mi, "ident") {
            build_ident(id, source)
        } else {
            Ident { name: "$".to_string(), span: span_of(mi) }
        }
    });

    let port = first_named_child_of_kind(node, "port_label").map(|pl| {
        if let Some(pr) = first_named_child_of_kind(pl, "param_ref") {
            if let Some(pri) = first_named_child_of_kind(pr, "param_ref_ident") {
                PortLabel::Param(build_ident(pri, source))
            } else {
                PortLabel::Param(Ident { name: String::new(), span: span_of(pr) })
            }
        } else if let Some(id) = first_named_child_of_kind(pl, "ident") {
            PortLabel::Literal(build_ident(id, source))
        } else {
            PortLabel::Literal(Ident { name: String::new(), span: span_of(pl) })
        }
    });

    let index = first_named_child_of_kind(node, "port_index")
        .map(|pi| build_port_index(pi, source, diags));

    PortRef {
        module,
        port,
        index,
        span: span_of(node),
    }
}

fn build_port_index(node: Node, source: &str, diags: &mut Vec<Diagnostic>) -> PortIndex {
    walk_errors(node, diags);

    if let Some(arity) = first_named_child_of_kind(node, "port_index_arity") {
        if let Some(id) = first_named_child_of_kind(arity, "ident") {
            return PortIndex::Arity(node_text(id, source).to_string());
        }
    }
    if let Some(nat) = first_named_child_of_kind(node, "nat") {
        if let Ok(n) = node_text(nat, source).parse::<u32>() {
            return PortIndex::Literal(n);
        }
    }
    if let Some(pr) = first_named_child_of_kind(node, "param_ref") {
        if let Some(pri) = first_named_child_of_kind(pr, "param_ref_ident") {
            return PortIndex::Alias(node_text(pri, source).to_string());
        }
    }
    if let Some(id) = first_named_child_of_kind(node, "ident") {
        return PortIndex::Alias(node_text(id, source).to_string());
    }

    PortIndex::Literal(0)
}

fn build_arrow(node: Node, source: &str, diags: &mut Vec<Diagnostic>) -> Arrow {
    walk_errors(node, diags);

    let mut direction = None;
    let mut scale = None;

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if !child.is_named() {
            continue;
        }
        match child.kind() {
            "forward_arrow" => {
                direction = Some(Direction::Forward);
                scale = first_named_child_of_kind(child, "scale_val")
                    .and_then(|sv| build_scale_val(sv, source, diags));
            }
            "backward_arrow" => {
                direction = Some(Direction::Backward);
                scale = first_named_child_of_kind(child, "scale_val")
                    .and_then(|sv| build_scale_val(sv, source, diags));
            }
            _ => {}
        }
    }

    Arrow {
        direction,
        scale,
        span: span_of(node),
    }
}

fn build_scale_val(node: Node, source: &str, diags: &mut Vec<Diagnostic>) -> Option<Scalar> {
    walk_errors(node, diags);

    if let Some(pr) = first_named_child_of_kind(node, "param_ref") {
        if let Some(pri) = first_named_child_of_kind(pr, "param_ref_ident") {
            return Some(Scalar::ParamRef(build_ident(pri, source)));
        }
    }
    if let Some(sn) = first_named_child_of_kind(node, "scale_num") {
        let text = node_text(sn, source);
        if let Ok(f) = text.parse::<f64>() {
            if text.contains('.') {
                return Some(Scalar::Float(f));
            } else {
                return Some(Scalar::Int(f as i64));
            }
        }
    }
    None
}

// ─── Template ───────────────────────────────────────────────────────────────

fn build_template(node: Node, source: &str, diags: &mut Vec<Diagnostic>) -> Template {
    walk_errors(node, diags);

    let name = node.child_by_field_name("name").map(|n| build_ident(n, source));

    let params = first_named_child_of_kind(node, "param_decls")
        .map(|pd| build_param_decls(pd, source, diags))
        .unwrap_or_default();

    let (in_ports, out_ports) = first_named_child_of_kind(node, "port_decls")
        .map(|pd| build_port_decls(pd, source, diags))
        .unwrap_or_default();

    let body = named_children_of_kind(node, "statement")
        .into_iter()
        .filter_map(|s| build_statement(s, source, diags))
        .collect();

    Template {
        name,
        params,
        in_ports,
        out_ports,
        body,
        span: span_of(node),
    }
}

fn build_param_decls(node: Node, source: &str, diags: &mut Vec<Diagnostic>) -> Vec<ParamDecl> {
    walk_errors(node, diags);
    named_children_of_kind(node, "param_decl")
        .into_iter()
        .map(|n| build_param_decl(n, source, diags))
        .collect()
}

fn build_param_decl(node: Node, source: &str, diags: &mut Vec<Diagnostic>) -> ParamDecl {
    walk_errors(node, diags);

    let idents = named_children_of_kind(node, "ident");
    let name = idents.first().map(|n| build_ident(*n, source));
    // If there are two idents, the second is the arity annotation (inside brackets)
    let arity = idents.get(1).map(|n| node_text(*n, source).to_string());

    let ty = first_named_child_of_kind(node, "type_name").map(|tn| {
        match node_text(tn, source) {
            "float" => ParamType::Float,
            "int" => ParamType::Int,
            "bool" => ParamType::Bool,
            "str" => ParamType::Str,
            _ => ParamType::Float,
        }
    });

    let default = first_named_child_of_kind(node, "scalar")
        .map(|s| build_scalar(s, source, diags));

    ParamDecl {
        name,
        arity,
        ty,
        default,
        span: span_of(node),
    }
}

fn build_port_decls(
    node: Node,
    source: &str,
    diags: &mut Vec<Diagnostic>,
) -> (Vec<PortGroupDecl>, Vec<PortGroupDecl>) {
    walk_errors(node, diags);

    let in_ports = first_named_child_of_kind(node, "in_decl")
        .map(|ind| {
            named_children_of_kind(ind, "port_group_decl")
                .into_iter()
                .map(|n| build_port_group_decl(n, source, diags))
                .collect()
        })
        .unwrap_or_default();

    let out_ports = first_named_child_of_kind(node, "out_decl")
        .map(|outd| {
            named_children_of_kind(outd, "port_group_decl")
                .into_iter()
                .map(|n| build_port_group_decl(n, source, diags))
                .collect()
        })
        .unwrap_or_default();

    (in_ports, out_ports)
}

fn build_port_group_decl(node: Node, source: &str, diags: &mut Vec<Diagnostic>) -> PortGroupDecl {
    walk_errors(node, diags);

    let idents = named_children_of_kind(node, "ident");
    let name = idents.first().map(|n| build_ident(*n, source));
    let arity = idents.get(1).map(|n| node_text(*n, source).to_string());

    PortGroupDecl {
        name,
        arity,
        span: span_of(node),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::language;

    fn parse(source: &str) -> (File, Vec<Diagnostic>) {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language()).unwrap();
        let tree = parser.parse(source, None).unwrap();
        build_ast(&tree, source)
    }

    #[test]
    fn valid_file_produces_zero_diagnostics() {
        let source = r#"
patch {
    module osc : Osc { frequency: 440Hz }
    module out : AudioOut

    osc.sine -> out.in_left
}
"#;
        let (file, diags) = parse(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let patch = file.patch.expect("patch should exist");
        assert_eq!(patch.body.len(), 3);

        // Verify module declarations
        match &patch.body[0] {
            Statement::Module(m) => {
                assert_eq!(m.name.as_ref().unwrap().name, "osc");
                assert_eq!(m.type_name.as_ref().unwrap().name, "Osc");
                assert_eq!(m.params.len(), 1);
            }
            _ => panic!("expected module"),
        }
    }

    #[test]
    fn missing_module_type_name() {
        let source = r#"
patch {
    module osc :
}
"#;
        let (file, diags) = parse(source);
        assert!(!diags.is_empty(), "expected diagnostics for missing type");
        let patch = file.patch.expect("patch should exist");
        // The module should still be parsed with name but no type
        if let Some(Statement::Module(m)) = patch.body.first() {
            assert_eq!(m.name.as_ref().unwrap().name, "osc");
        }
    }

    #[test]
    fn unclosed_param_block() {
        let source = r#"
patch {
    module osc : Osc { frequency: 440
}
"#;
        let (_, diags) = parse(source);
        assert!(!diags.is_empty(), "expected diagnostics for unclosed block");
    }

    #[test]
    fn template_with_params_and_ports() {
        let source = r#"
template voice(attack: float = 0.01) {
    in:  voct, gate
    out: audio

    module osc : Osc
    module env : Adsr { attack: <attack> }
    module vca : Vca

    osc.sine -> vca.in
    env.out  -> vca.cv
}

patch {
    module v : voice
    module out : AudioOut
    v.audio -> out.in_left
}
"#;
        let (file, diags) = parse(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(file.templates.len(), 1);
        let tmpl = &file.templates[0];
        assert_eq!(tmpl.name.as_ref().unwrap().name, "voice");
        assert_eq!(tmpl.params.len(), 1);
        assert_eq!(tmpl.in_ports.len(), 2);
        assert_eq!(tmpl.out_ports.len(), 1);
        assert_eq!(tmpl.body.len(), 5);
    }

    #[test]
    fn connection_with_scale() {
        let source = r#"
patch {
    module a : Osc
    module b : Vca
    a.out -[0.5]-> b.in
}
"#;
        let (file, diags) = parse(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let patch = file.patch.unwrap();
        if let Statement::Connection(conn) = &patch.body[2] {
            let arrow = conn.arrow.as_ref().unwrap();
            assert_eq!(arrow.direction, Some(Direction::Forward));
            assert_eq!(arrow.scale, Some(Scalar::Float(0.5)));
        } else {
            panic!("expected connection");
        }
    }

    #[test]
    fn backward_arrow() {
        let source = r#"
patch {
    module a : Osc
    module b : Vca
    b.in <- a.out
}
"#;
        let (file, diags) = parse(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let patch = file.patch.unwrap();
        if let Statement::Connection(conn) = &patch.body[2] {
            let arrow = conn.arrow.as_ref().unwrap();
            assert_eq!(arrow.direction, Some(Direction::Backward));
        } else {
            panic!("expected connection");
        }
    }

    #[test]
    fn note_literal_conversion() {
        let source = r#"
patch {
    module osc : Osc { freq: C4 }
}
"#;
        let (file, diags) = parse(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let patch = file.patch.unwrap();
        if let Statement::Module(m) = &patch.body[0] {
            if let ParamEntry::KeyValue { value: Some(Value::Scalar(Scalar::Float(v))), .. } =
                &m.params[0]
            {
                // C4 = (4*12 + 0) / 12 = 4.0
                assert!((v - 4.0).abs() < 1e-10, "expected 4.0, got {v}");
            } else {
                panic!("expected float scalar, got: {:?}", m.params[0]);
            }
        }
    }

    #[test]
    fn hz_unit_conversion() {
        let source = r#"
patch {
    module osc : Osc { frequency: 440Hz }
}
"#;
        let (file, diags) = parse(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let patch = file.patch.unwrap();
        if let Statement::Module(m) = &patch.body[0] {
            if let ParamEntry::KeyValue { value: Some(Value::Scalar(Scalar::Float(v))), .. } =
                &m.params[0]
            {
                // 440 Hz = log2(440/C0_HZ) v/oct
                let expected = (440.0_f64 / C0_HZ).log2();
                assert!((v - expected).abs() < 1e-10, "expected {expected}, got {v}");
            } else {
                panic!("expected float scalar");
            }
        }
    }

    #[test]
    fn db_unit_conversion() {
        let source = r#"
patch {
    module mix : Mixer { level: -6dB }
}
"#;
        let (file, diags) = parse(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let patch = file.patch.unwrap();
        if let Statement::Module(m) = &patch.body[0] {
            if let ParamEntry::KeyValue { value: Some(Value::Scalar(Scalar::Float(v))), .. } =
                &m.params[0]
            {
                let expected = 10.0_f64.powf(-6.0 / 20.0);
                assert!((v - expected).abs() < 1e-10, "expected {expected}, got {v}");
            } else {
                panic!("expected float scalar");
            }
        }
    }

    #[test]
    fn at_block_parsing() {
        let source = r#"
patch {
    module del : StereoDelay(channels: [tap1, tap2]) {
        @tap1: { delay_ms: 700, feedback: 0.3 }
        @tap2: { delay_ms: 450, feedback: 0.3 }
    }
}
"#;
        let (file, diags) = parse(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let patch = file.patch.unwrap();
        if let Statement::Module(m) = &patch.body[0] {
            assert_eq!(m.params.len(), 2);
            if let ParamEntry::AtBlock { index, entries, .. } = &m.params[0] {
                assert_eq!(*index, Some(AtBlockIndex::Alias("tap1".to_string())));
                assert_eq!(entries.len(), 2);
            } else {
                panic!("expected at-block");
            }
        }
    }

    #[test]
    fn parses_all_fixture_files() {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language()).unwrap();

        let fixture_dirs = [
            concat!(env!("CARGO_MANIFEST_DIR"), "/../patches-dsl/tests/fixtures"),
            concat!(env!("CARGO_MANIFEST_DIR"), "/../examples"),
        ];
        let mut count = 0;
        for dir in &fixture_dirs {
            for entry in std::fs::read_dir(dir).expect(dir) {
                let path = entry.unwrap().path();
                if path.extension().is_some_and(|e| e == "patches") {
                    let source = std::fs::read_to_string(&path).unwrap();
                    let tree = parser.parse(&source, None).unwrap();
                    let (file, diags) = build_ast(&tree, &source);
                    assert!(
                        diags.is_empty(),
                        "{}: unexpected diagnostics: {diags:?}",
                        path.display()
                    );
                    assert!(file.patch.is_some(), "{}: no patch block", path.display());
                    count += 1;
                }
            }
        }
        assert!(count >= 10, "expected at least 10 fixture files, found {count}");
    }

    #[test]
    fn shape_args_with_alias_list() {
        let source = r#"
patch {
    module mix : Mixer(channels: [drums, bass, synth])
}
"#;
        let (file, diags) = parse(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let patch = file.patch.unwrap();
        if let Statement::Module(m) = &patch.body[0] {
            assert_eq!(m.shape.len(), 1);
            let sa = &m.shape[0];
            assert_eq!(sa.name.as_ref().unwrap().name, "channels");
            if let Some(ShapeArgValue::AliasList(aliases)) = &sa.value {
                assert_eq!(aliases.len(), 3);
                assert_eq!(aliases[0].name, "drums");
            } else {
                panic!("expected alias list");
            }
        }
    }

    #[test]
    fn at_block_without_colon() {
        let source = r#"
patch {
    module mixer : Mixer(channels: [drum, bass]) {
        @drum { level: 1.0 }
        @bass { level: 0.5 }
    }
}
"#;
        let (file, diags) = parse(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let patch = file.patch.unwrap();
        if let Statement::Module(m) = &patch.body[0] {
            // Should have 2 at-block params
            assert_eq!(m.params.len(), 2, "expected 2 at-block params: {:?}", m.params);
        } else {
            panic!("expected module");
        }
    }

    #[test]
    fn port_ref_with_dollar() {
        let source = r#"
template v {
    in:  input
    out: output

    module osc : Osc
    $.input -> osc.voct
    osc.sine -> $.output
}

patch {
    module v1 : v
}
"#;
        let (file, diags) = parse(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let tmpl = &file.templates[0];
        if let Statement::Connection(conn) = &tmpl.body[1] {
            assert_eq!(conn.lhs.as_ref().unwrap().module.as_ref().unwrap().name, "$");
        } else {
            panic!("expected connection");
        }
    }
}
