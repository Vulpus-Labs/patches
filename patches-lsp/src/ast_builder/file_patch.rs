use crate::ast::{
    Arrow, Connection, Direction, File, ParamDecl, ParamType, Patch, PortGroupDecl, PortIndex,
    PortLabel, PortRef, Scalar, Statement, Template,
};
use crate::lsp_util::first_named_child_of_kind;
use super::{named_children_of_kind, node_text, span_of, build_ident};
use super::diagnostics::{Diagnostic, walk_errors};
use super::literals::build_scalar;
use super::module_decl::build_module_decl;

pub(super) fn build_include_directive(node: tree_sitter::Node, source: &str) -> Option<crate::ast::IncludeDirective> {
    let path_node = node.child_by_field_name("path")?;
    let raw = node_text(path_node, source);
    // Strip surrounding quotes from string_lit.
    let path = if raw.len() >= 2 { raw[1..raw.len() - 1].to_owned() } else { raw.to_owned() };
    Some(crate::ast::IncludeDirective {
        path,
        span: span_of(node),
    })
}

pub(super) fn build_file(node: tree_sitter::Node, source: &str, diags: &mut Vec<Diagnostic>) -> File {
    walk_errors(node, diags);

    let mut includes = Vec::new();
    let mut templates = Vec::new();
    let mut patterns = Vec::new();
    let mut songs = Vec::new();
    let mut patch = None;

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "include_directive" => {
                if let Some(inc) = build_include_directive(child, source) {
                    includes.push(inc);
                }
            }
            "template" => templates.push(build_template(child, source, diags)),
            "pattern_block" => patterns.push(super::song_pattern::build_pattern_block(child, source, diags)),
            "song_block" => songs.push(super::song_pattern::build_song_block(child, source, diags)),
            "patch" => patch = Some(build_patch(child, source, diags)),
            _ => {}
        }
    }

    File {
        includes,
        templates,
        patterns,
        songs,
        patch,
        span: span_of(node),
    }
}

pub(super) fn build_patch(node: tree_sitter::Node, source: &str, diags: &mut Vec<Diagnostic>) -> Patch {
    walk_errors(node, diags);

    let body = named_children_of_kind(node, "statement")
        .into_iter()
        .flat_map(|s| build_statements(s, source, diags))
        .collect();

    Patch {
        body,
        span: span_of(node),
    }
}

pub(super) fn build_statements(node: tree_sitter::Node, source: &str, diags: &mut Vec<Diagnostic>) -> Vec<Statement> {
    walk_errors(node, diags);

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if !child.is_named() {
            continue;
        }
        match child.kind() {
            "module_decl" => return vec![Statement::Module(build_module_decl(child, source, diags))],
            "connection" => return build_connections(child, source, diags)
                .into_iter()
                .map(|c| Statement::Connection(Box::new(c)))
                .collect(),
            _ => {}
        }
    }
    vec![]
}

fn build_connections(node: tree_sitter::Node, source: &str, diags: &mut Vec<Diagnostic>) -> Vec<Connection> {
    walk_errors(node, diags);

    // The grammar wraps each endpoint in a `cable_endpoint` node (parity
    // with the canonical pest grammar). Descend through it to find the
    // inner `port_ref`. Endpoints that are `tap_target` or
    // `host_control_ref` are silently skipped here — the LSP's connection
    // model only carries port references; richer endpoints are handled
    // by other passes.
    let port_refs: Vec<tree_sitter::Node> = named_children_of_kind(node, "cable_endpoint")
        .into_iter()
        .filter_map(|ce| first_named_child_of_kind(ce, "port_ref"))
        .collect();
    let lhs = port_refs.first().map(|n| build_port_ref(*n, source, diags));
    let arrow = first_named_child_of_kind(node, "arrow")
        .map(|n| build_arrow(n, source, diags));
    let span = span_of(node);

    let rhs_refs = port_refs.get(1..).unwrap_or(&[]);
    if rhs_refs.is_empty() {
        // Incomplete parse — emit a single connection with no RHS
        return vec![Connection { lhs, arrow, rhs: None, span }];
    }

    rhs_refs.iter().map(|n| {
        Connection {
            lhs: lhs.clone(),
            arrow: arrow.clone(),
            rhs: Some(build_port_ref(*n, source, diags)),
            span,
        }
    }).collect()
}

fn build_port_ref(node: tree_sitter::Node, source: &str, diags: &mut Vec<Diagnostic>) -> PortRef {
    walk_errors(node, diags);

    let module = first_named_child_of_kind(node, "module_ident").map(|mi| {
        // module_ident is either "$" or contains an ident child
        if let Some(id) = first_named_child_of_kind(mi, "ident") {
            build_ident(id, source)
        } else {
            crate::ast::Ident { name: "$".to_string(), span: span_of(mi) }
        }
    });

    let port = first_named_child_of_kind(node, "port_label").map(|pl| {
        if let Some(pr) = first_named_child_of_kind(pl, "param_ref") {
            if let Some(pri) = first_named_child_of_kind(pr, "param_ref_ident") {
                PortLabel::Param(build_ident(pri, source))
            } else {
                PortLabel::Param(crate::ast::Ident { name: String::new(), span: span_of(pr) })
            }
        } else if let Some(id) = first_named_child_of_kind(pl, "ident") {
            PortLabel::Literal(build_ident(id, source))
        } else {
            PortLabel::Literal(crate::ast::Ident { name: String::new(), span: span_of(pl) })
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

fn build_port_index(node: tree_sitter::Node, source: &str, diags: &mut Vec<Diagnostic>) -> PortIndex {
    walk_errors(node, diags);

    if let Some(arity) = first_named_child_of_kind(node, "port_index_arity") {
        if let Some(id) = first_named_child_of_kind(arity, "ident") {
            return PortIndex::Name {
                name: node_text(id, source).to_string(),
                arity_marker: true,
            };
        }
    }
    if let Some(nat) = first_named_child_of_kind(node, "nat") {
        if let Ok(n) = node_text(nat, source).parse::<u32>() {
            return PortIndex::Literal(n);
        }
    }
    if let Some(pr) = first_named_child_of_kind(node, "param_ref") {
        if let Some(pri) = first_named_child_of_kind(pr, "param_ref_ident") {
            return PortIndex::Name {
                name: node_text(pri, source).to_string(),
                arity_marker: false,
            };
        }
    }
    if let Some(id) = first_named_child_of_kind(node, "ident") {
        return PortIndex::Name {
            name: node_text(id, source).to_string(),
            arity_marker: false,
        };
    }

    PortIndex::Literal(0)
}

fn build_arrow(node: tree_sitter::Node, source: &str, diags: &mut Vec<Diagnostic>) -> Arrow {
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

fn build_scale_val(node: tree_sitter::Node, source: &str, diags: &mut Vec<Diagnostic>) -> Option<Scalar> {
    walk_errors(node, diags);

    // scale_val wraps either a scale_range or a scale_endpoint; for the
    // simple-scalar case we descend through scale_endpoint to find the
    // inner literal (param_ref / float_unit / note_lit / scale_num).
    let endpoint = first_named_child_of_kind(node, "scale_endpoint")?;
    if let Some(pr) = first_named_child_of_kind(endpoint, "param_ref") {
        if let Some(pri) = first_named_child_of_kind(pr, "param_ref_ident") {
            return Some(Scalar::ParamRef(build_ident(pri, source)));
        }
    }
    if let Some(sn) = first_named_child_of_kind(endpoint, "scale_num") {
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

pub(super) fn build_template(node: tree_sitter::Node, source: &str, diags: &mut Vec<Diagnostic>) -> Template {
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
        .flat_map(|s| build_statements(s, source, diags))
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

fn build_param_decls(node: tree_sitter::Node, source: &str, diags: &mut Vec<Diagnostic>) -> Vec<ParamDecl> {
    walk_errors(node, diags);
    named_children_of_kind(node, "param_decl")
        .into_iter()
        .map(|n| build_param_decl(n, source, diags))
        .collect()
}

fn build_param_decl(node: tree_sitter::Node, source: &str, diags: &mut Vec<Diagnostic>) -> ParamDecl {
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
    node: tree_sitter::Node,
    source: &str,
    diags: &mut Vec<Diagnostic>,
) -> (Vec<PortGroupDecl>, Vec<PortGroupDecl>) {
    walk_errors(node, diags);

    // in_decl / out_decl wrap their port-group list in `comma_port_decls`
    // (parity with pest). Descend through it to find each port_group_decl.
    let collect_groups = |decl: tree_sitter::Node, diags: &mut Vec<Diagnostic>| -> Vec<PortGroupDecl> {
        first_named_child_of_kind(decl, "comma_port_decls")
            .map(|cpd| {
                named_children_of_kind(cpd, "port_group_decl")
                    .into_iter()
                    .map(|n| build_port_group_decl(n, source, diags))
                    .collect()
            })
            .unwrap_or_default()
    };

    let in_ports = first_named_child_of_kind(node, "in_decl")
        .map(|ind| collect_groups(ind, diags))
        .unwrap_or_default();
    let out_ports = first_named_child_of_kind(node, "out_decl")
        .map(|outd| collect_groups(outd, diags))
        .unwrap_or_default();

    (in_ports, out_ports)
}

fn build_port_group_decl(node: tree_sitter::Node, source: &str, diags: &mut Vec<Diagnostic>) -> PortGroupDecl {
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
