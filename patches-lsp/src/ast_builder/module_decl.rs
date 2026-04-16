use crate::ast::{ModuleDecl, ShapeArg, ShapeArgValue};
use crate::lsp_util::first_named_child_of_kind;
use super::{named_children_of_kind, span_of, build_ident};
use super::diagnostics::{Diagnostic, walk_errors};
use super::literals::build_scalar;
use super::params::build_param_block;

pub(super) fn build_module_decl(node: tree_sitter::Node, source: &str, diags: &mut Vec<Diagnostic>) -> ModuleDecl {
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

pub(super) fn build_shape_block(node: tree_sitter::Node, source: &str, diags: &mut Vec<Diagnostic>) -> Vec<ShapeArg> {
    walk_errors(node, diags);
    named_children_of_kind(node, "shape_arg")
        .into_iter()
        .map(|n| build_shape_arg(n, source, diags))
        .collect()
}

fn build_shape_arg(node: tree_sitter::Node, source: &str, diags: &mut Vec<Diagnostic>) -> ShapeArg {
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

fn build_alias_list(node: tree_sitter::Node, source: &str, diags: &mut Vec<Diagnostic>) -> ShapeArgValue {
    walk_errors(node, diags);
    let idents = named_children_of_kind(node, "ident")
        .into_iter()
        .map(|n| build_ident(n, source))
        .collect();
    ShapeArgValue::AliasList(idents)
}
