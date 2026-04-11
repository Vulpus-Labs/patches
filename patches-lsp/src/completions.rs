//! Completion engine for the patches DSL.
//!
//! Provides context-sensitive completions for module types, parameters,
//! ports, shape arguments, and template ports.

use patches_core::Registry;
use tower_lsp::lsp_types::*;
use tree_sitter::Tree;

use crate::analysis::{self, ResolvedDescriptor, SemanticModel};
use crate::lsp_util::{find_ancestor, first_named_child_of_kind, node_text};

// ─── Public entry point ──────────────────────────────────────────────────

/// Determine the completion context from the cursor position and return items.
pub(crate) fn compute_completions(
    tree: &Tree,
    source: &str,
    byte_offset: usize,
    model: &SemanticModel,
    registry: &Registry,
) -> Vec<CompletionItem> {
    // Try the node at the cursor, and also one byte back (the cursor often sits
    // just past the last character of a token).
    let root = tree.root_node();
    let offsets: &[usize] = if byte_offset > 0 {
        &[byte_offset, byte_offset - 1]
    } else {
        &[byte_offset]
    };

    for &off in offsets {
        let node = match root.descendant_for_byte_range(off, off) {
            Some(n) => n,
            None => continue,
        };

        if let Some(items) =
            try_completion_from_node(node, source, byte_offset, tree, model, registry)
        {
            return items;
        }
    }

    // Fallback: check if we're in a text context that looks like `module name : |`
    // or `module name : Partial` by scanning backward from cursor.
    if let Some(ctx) = scan_backward_for_context(source, byte_offset) {
        match ctx {
            BackwardContext::ModuleColon | BackwardContext::ModuleTypeName => {
                return complete_module_types(model, registry);
            }
            BackwardContext::Dot(module_name) => {
                return complete_ports(&module_name, model, byte_offset, source, tree);
            }
            BackwardContext::DollarDot => {
                return complete_template_ports(source, byte_offset, tree, model);
            }
            BackwardContext::PortIndex(module_name) => {
                return complete_port_index_aliases(&module_name, model);
            }
            BackwardContext::SongRow => {
                return complete_pattern_names(model);
            }
        }
    }

    vec![]
}

// ─── Node-based completion ───────────────────────────────────────────────

/// Try to determine completion context from a given tree-sitter node.
fn try_completion_from_node(
    node: tree_sitter::Node,
    source: &str,
    byte_offset: usize,
    tree: &Tree,
    model: &SemanticModel,
    registry: &Registry,
) -> Option<Vec<CompletionItem>> {
    let mut cursor = node;
    loop {
        match cursor.kind() {
            "module_decl" => {
                if is_after_colon_in_module_decl(source, byte_offset, cursor) {
                    return Some(complete_module_types(model, registry));
                }
                if is_inside_child_kind(node, "param_block") {
                    if is_after_at_sign(source, byte_offset) {
                        return Some(complete_at_block_aliases(cursor, source));
                    }
                    // Check if this is a MasterSequencer song: param
                    let module_type = cursor
                        .child_by_field_name("type")
                        .map(|n| node_text(n, source));
                    if module_type == Some("MasterSequencer")
                        && is_after_param_colon(source, byte_offset, "song")
                    {
                        return Some(complete_song_names(model));
                    }
                    let module_name = cursor
                        .child_by_field_name("name")
                        .map(|n| node_text(n, source));
                    return Some(complete_parameters(module_name, model));
                }
                if is_inside_child_kind(node, "shape_block") {
                    return Some(complete_shape_args());
                }
                return None;
            }
            "param_block" => {
                if let Some(module_decl) = find_ancestor(cursor, "module_decl") {
                    if is_after_at_sign(source, byte_offset) {
                        return Some(complete_at_block_aliases(module_decl, source));
                    }
                    // Check if this is a MasterSequencer song: param
                    let module_type = module_decl
                        .child_by_field_name("type")
                        .map(|n| node_text(n, source));
                    if module_type == Some("MasterSequencer")
                        && is_after_param_colon(source, byte_offset, "song")
                    {
                        return Some(complete_song_names(model));
                    }
                    let module_name = module_decl
                        .child_by_field_name("name")
                        .map(|n| node_text(n, source));
                    return Some(complete_parameters(module_name, model));
                }
                return None;
            }
            "shape_block" => {
                return Some(complete_shape_args());
            }
            "port_ref" | "connection" => {
                return Some(complete_port_ref(source, byte_offset, node, tree, model));
            }
            "song_row" | "song_cell" => {
                return Some(complete_pattern_names(model));
            }
            _ => {}
        }
        cursor = cursor.parent()?;
    }
}

// ─── Completion helpers ──────────────────────────────────────────────────

/// Check if the cursor is positioned after the `:` in a module_decl (type position).
fn is_after_colon_in_module_decl(
    _source: &str,
    byte_offset: usize,
    module_decl: tree_sitter::Node,
) -> bool {
    let mut child_cursor = module_decl.walk();
    let mut found_colon = false;
    for child in module_decl.children(&mut child_cursor) {
        if child.kind() == ":" && child.end_byte() <= byte_offset {
            found_colon = true;
        }
    }
    if !found_colon {
        return false;
    }

    let at_type = module_decl
        .child_by_field_name("type")
        .is_none_or(|t| byte_offset <= t.end_byte());
    if at_type {
        return true;
    }

    let no_shape = first_named_child_of_kind(module_decl, "shape_block")
        .is_none_or(|s| byte_offset < s.start_byte());
    let no_params = first_named_child_of_kind(module_decl, "param_block")
        .is_none_or(|p| byte_offset < p.start_byte());
    no_shape && no_params
}

/// Check if the given node or any of its ancestors is inside a child of the given kind.
fn is_inside_child_kind(node: tree_sitter::Node, kind: &str) -> bool {
    let mut cursor = node;
    loop {
        if cursor.kind() == kind {
            return true;
        }
        cursor = match cursor.parent() {
            Some(p) => p,
            None => return false,
        };
    }
}

/// Complete with all registered module type names and template names.
fn complete_module_types(model: &SemanticModel, registry: &Registry) -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = registry
        .module_names()
        .map(|name| CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::MODULE),
            ..Default::default()
        })
        .collect();

    for name in model.declarations.templates.keys() {
        items.push(CompletionItem {
            label: name.clone(),
            kind: Some(CompletionItemKind::MODULE),
            detail: Some("template".to_string()),
            ..Default::default()
        });
    }

    items.sort_by(|a, b| a.label.cmp(&b.label));
    items
}

/// Complete with shape argument names.
fn complete_shape_args() -> Vec<CompletionItem> {
    ["channels", "length", "high_quality"]
        .iter()
        .map(|name| CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::PROPERTY),
            ..Default::default()
        })
        .collect()
}

/// Check if the cursor is immediately after an `@` sign.
fn is_after_at_sign(source: &str, byte_offset: usize) -> bool {
    let before = &source[..byte_offset];
    let trimmed = before.trim_end_matches(|c: char| c.is_alphanumeric() || c == '_');
    trimmed.ends_with('@')
}

/// Complete with shape alias names for a port index (inside `[...]`).
fn complete_port_index_aliases(module_name: &str, model: &SemanticModel) -> Vec<CompletionItem> {
    for module in &model.declarations.modules {
        if module.name == module_name {
            return shape_aliases_from_args(&module.shape_args);
        }
    }
    vec![]
}

/// Extract alias names from shape args as completion items.
fn shape_aliases_from_args(shape_args: &[(String, analysis::ShapeValue)]) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    for (_, value) in shape_args {
        if let analysis::ShapeValue::AliasList(aliases) = value {
            for alias in aliases {
                items.push(CompletionItem {
                    label: alias.clone(),
                    kind: Some(CompletionItemKind::ENUM_MEMBER),
                    ..Default::default()
                });
            }
        }
    }
    items
}

fn complete_at_block_aliases(
    module_decl: tree_sitter::Node,
    source: &str,
) -> Vec<CompletionItem> {
    let shape_block = match first_named_child_of_kind(module_decl, "shape_block") {
        Some(sb) => sb,
        None => return vec![],
    };
    let mut items = Vec::new();
    let mut cursor = shape_block.walk();
    for shape_arg in shape_block.named_children(&mut cursor) {
        if shape_arg.kind() != "shape_arg" {
            continue;
        }
        if let Some(alias_list) = first_named_child_of_kind(shape_arg, "alias_list") {
            let mut alias_cursor = alias_list.walk();
            for ident in alias_list.named_children(&mut alias_cursor) {
                if ident.kind() == "ident" {
                    items.push(CompletionItem {
                        label: node_text(ident, source).to_string(),
                        kind: Some(CompletionItemKind::ENUM_MEMBER),
                        ..Default::default()
                    });
                }
            }
        }
    }
    items
}

/// Complete with parameter names for a module.
fn complete_parameters(
    module_name: Option<&str>,
    model: &SemanticModel,
) -> Vec<CompletionItem> {
    let module_name = match module_name {
        Some(n) => n,
        None => return vec![],
    };
    let desc = match model.get_descriptor(module_name) {
        Some(d) => d,
        None => return vec![],
    };
    match desc {
        ResolvedDescriptor::Module(md) => {
            let mut seen = std::collections::HashSet::new();
            md.parameters
                .iter()
                .filter(|p| seen.insert(p.name))
                .map(|p| CompletionItem {
                    label: p.name.to_string(),
                    kind: Some(CompletionItemKind::PROPERTY),
                    detail: Some(format_parameter_kind(&p.parameter_type)),
                    ..Default::default()
                })
                .collect()
        }
        ResolvedDescriptor::Template { .. } => vec![],
    }
}

/// Complete port names for a port reference.
fn complete_port_ref(
    source: &str,
    byte_offset: usize,
    node: tree_sitter::Node,
    tree: &Tree,
    model: &SemanticModel,
) -> Vec<CompletionItem> {
    let port_ref_node = if node.kind() == "port_ref" {
        node
    } else {
        match find_ancestor(node, "port_ref") {
            Some(n) => n,
            None => return vec![],
        }
    };

    let module_ident = first_named_child_of_kind(port_ref_node, "module_ident");
    let module_name = module_ident.map(|n| {
        first_named_child_of_kind(n, "ident")
            .map(|id| node_text(id, source))
            .unwrap_or_else(|| node_text(n, source))
    });

    if let Some(name) = module_name {
        if name == "$" {
            return complete_template_ports(source, byte_offset, tree, model);
        }
        return complete_ports(name, model, byte_offset, source, tree);
    }

    vec![]
}

/// Complete ports for a named module. Determines whether to offer inputs or
/// outputs based on the connection context (source vs destination side).
fn complete_ports(
    module_name: &str,
    model: &SemanticModel,
    byte_offset: usize,
    source: &str,
    tree: &Tree,
) -> Vec<CompletionItem> {
    let desc = match model.get_descriptor(module_name) {
        Some(d) => d,
        None => return vec![],
    };

    let side = determine_connection_side(tree, source, byte_offset);

    match desc {
        ResolvedDescriptor::Module(md) => {
            let ports = match side {
                ConnectionSide::Source => &md.outputs,
                ConnectionSide::Destination => &md.inputs,
                ConnectionSide::Unknown => {
                    let mut items: Vec<CompletionItem> = md
                        .outputs
                        .iter()
                        .map(|p| CompletionItem {
                            label: p.name.to_string(),
                            kind: Some(CompletionItemKind::FIELD),
                            detail: Some(format!("output ({})", cable_kind_str(&p.kind))),
                            ..Default::default()
                        })
                        .collect();
                    items.extend(md.inputs.iter().map(|p| CompletionItem {
                        label: p.name.to_string(),
                        kind: Some(CompletionItemKind::FIELD),
                        detail: Some(format!("input ({})", cable_kind_str(&p.kind))),
                        ..Default::default()
                    }));
                    dedup_completion_items(&mut items);
                    return items;
                }
            };
            let mut items: Vec<CompletionItem> = ports
                .iter()
                .map(|p| CompletionItem {
                    label: p.name.to_string(),
                    kind: Some(CompletionItemKind::FIELD),
                    detail: Some(cable_kind_str(&p.kind).to_string()),
                    ..Default::default()
                })
                .collect();
            dedup_completion_items(&mut items);
            items
        }
        ResolvedDescriptor::Template {
            in_ports,
            out_ports,
        } => {
            let ports = match side {
                ConnectionSide::Source => out_ports,
                ConnectionSide::Destination => in_ports,
                ConnectionSide::Unknown => {
                    let mut items: Vec<CompletionItem> = out_ports
                        .iter()
                        .chain(in_ports.iter())
                        .map(|name| CompletionItem {
                            label: name.clone(),
                            kind: Some(CompletionItemKind::FIELD),
                            ..Default::default()
                        })
                        .collect();
                    dedup_completion_items(&mut items);
                    return items;
                }
            };
            ports
                .iter()
                .map(|name| CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::FIELD),
                    ..Default::default()
                })
                .collect()
        }
    }
}

/// Complete template external ports (after `$.`).
fn complete_template_ports(
    source: &str,
    byte_offset: usize,
    tree: &Tree,
    model: &SemanticModel,
) -> Vec<CompletionItem> {
    let root = tree.root_node();
    let node = match root.descendant_for_byte_range(byte_offset, byte_offset) {
        Some(n) => n,
        None => return vec![],
    };

    let template_node = if node.kind() == "template" {
        node
    } else {
        match find_ancestor(node, "template") {
            Some(n) => n,
            None => return vec![],
        }
    };

    let tmpl_name = template_node
        .child_by_field_name("name")
        .map(|n| node_text(n, source));

    if let Some(name) = tmpl_name {
        if let Some(info) = model.declarations.templates.get(name) {
            let mut items: Vec<CompletionItem> = info
                .in_ports
                .iter()
                .map(|p| CompletionItem {
                    label: p.name.clone(),
                    kind: Some(CompletionItemKind::FIELD),
                    detail: Some("in port".to_string()),
                    ..Default::default()
                })
                .collect();
            items.extend(info.out_ports.iter().map(|p| CompletionItem {
                label: p.name.clone(),
                kind: Some(CompletionItemKind::FIELD),
                detail: Some("out port".to_string()),
                ..Default::default()
            }));
            return items;
        }
    }

    vec![]
}

/// Check if cursor is positioned after `param_name:` in a param block.
fn is_after_param_colon(source: &str, byte_offset: usize, param_name: &str) -> bool {
    let before = &source[..byte_offset];
    let trimmed = before.trim_end();
    // Look for `param_name:` possibly with whitespace
    let pattern = format!("{param_name}:");
    if let Some(pos) = trimmed.rfind(&pattern) {
        // Ensure nothing between the colon and cursor except whitespace/partial ident
        let after_colon = &trimmed[pos + pattern.len()..];
        let after_colon = after_colon.trim_start();
        // Either empty (just after colon) or a partial identifier
        after_colon.is_empty()
            || after_colon
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    } else {
        false
    }
}

/// Complete with all defined pattern names.
fn complete_pattern_names(model: &SemanticModel) -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = model
        .declarations
        .patterns
        .values()
        .map(|p| CompletionItem {
            label: p.name.clone(),
            kind: Some(CompletionItemKind::REFERENCE),
            detail: Some(format!(
                "pattern \u{2014} {} channels, {} steps",
                p.channel_count, p.step_count
            )),
            ..Default::default()
        })
        .collect();
    items.sort_by(|a, b| a.label.cmp(&b.label));
    items
}

/// Complete with all defined song names.
fn complete_song_names(model: &SemanticModel) -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = model
        .declarations
        .songs
        .values()
        .map(|s| CompletionItem {
            label: s.name.clone(),
            kind: Some(CompletionItemKind::REFERENCE),
            detail: Some(format!(
                "song \u{2014} {} channels, {} rows",
                s.channel_names.len(),
                s.rows.len()
            )),
            ..Default::default()
        })
        .collect();
    items.sort_by(|a, b| a.label.cmp(&b.label));
    items
}

enum ConnectionSide {
    Source,
    Destination,
    Unknown,
}

/// Determine whether a port ref at the given offset is on the source or
/// destination side of a connection.
fn determine_connection_side(
    tree: &Tree,
    source: &str,
    byte_offset: usize,
) -> ConnectionSide {
    let root = tree.root_node();
    let node = match root.descendant_for_byte_range(byte_offset, byte_offset) {
        Some(n) => n,
        None => return ConnectionSide::Unknown,
    };

    let conn_node = match find_ancestor(node, "connection").or_else(|| {
        if node.kind() == "connection" {
            Some(node)
        } else {
            None
        }
    }) {
        Some(n) => n,
        None => return ConnectionSide::Unknown,
    };

    let arrow_node = first_named_child_of_kind(conn_node, "arrow");
    let is_forward = arrow_node
        .map(|a| {
            let text = node_text(a, source);
            text.contains("->")
        })
        .unwrap_or(true);

    let arrow_start = arrow_node.map(|a| a.start_byte()).unwrap_or(byte_offset);

    if byte_offset < arrow_start {
        if is_forward {
            ConnectionSide::Source
        } else {
            ConnectionSide::Destination
        }
    } else if is_forward {
        ConnectionSide::Destination
    } else {
        ConnectionSide::Source
    }
}

/// Context determined by scanning backward from cursor position.
enum BackwardContext {
    ModuleColon,
    ModuleTypeName,
    Dot(String),
    DollarDot,
    /// `module_name.port_name[` — complete with shape aliases
    PortIndex(String),
    /// Inside a song block row (after `|`)
    SongRow,
}

/// Scan backward from the cursor to determine context when tree-sitter nodes
/// don't give enough information (e.g. in incomplete input).
fn scan_backward_for_context(source: &str, byte_offset: usize) -> Option<BackwardContext> {
    let before = &source[..byte_offset];
    let trimmed = before.trim_end();

    // Check for `module.port[` — complete with shape aliases for the module.
    if let Some(before_bracket) = trimmed.strip_suffix('[') {
        let before_bracket = before_bracket.trim_end();
        if let Some(dot_pos) = before_bracket.rfind('.') {
            let before_dot = &before_bracket[..dot_pos];
            let module_name = before_dot
                .rsplit(|c: char| c.is_whitespace() || c == '{' || c == '}')
                .next()?;
            if !module_name.is_empty()
                && module_name
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
            {
                return Some(BackwardContext::PortIndex(module_name.to_string()));
            }
        }
    }

    if trimmed.ends_with("$.") {
        return Some(BackwardContext::DollarDot);
    }

    if let Some(before_dot) = trimmed.strip_suffix('.') {
        let before_dot = before_dot.trim_end();
        let module_name = before_dot
            .rsplit(|c: char| c.is_whitespace() || c == '{' || c == '}')
            .next()?;
        if !module_name.is_empty()
            && module_name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Some(BackwardContext::Dot(module_name.to_string()));
        }
    }

    // Check for `module <name> : |` pattern.
    if let Some(before_colon) = trimmed.strip_suffix(':') {
        let before_colon = before_colon.trim_end();
        let parts: Vec<&str> = before_colon.rsplitn(3, char::is_whitespace).collect();
        if parts.len() >= 2 && (parts[1] == "module" || parts.get(2) == Some(&"module")) {
            return Some(BackwardContext::ModuleColon);
        }
    }

    // Check for `module <name> : <PartialTypeName>` — cursor is mid-type-name.
    let word_start = trimmed
        .rfind(|c: char| c.is_whitespace())
        .map(|i| i + 1)
        .unwrap_or(0);
    let before_word = trimmed[..word_start].trim_end();
    if let Some(before_colon) = before_word.strip_suffix(':') {
        let before_colon = before_colon.trim_end();
        let parts: Vec<&str> = before_colon.rsplitn(3, char::is_whitespace).collect();
        if parts.len() >= 2 && (parts[1] == "module" || parts.get(2) == Some(&"module")) {
            return Some(BackwardContext::ModuleTypeName);
        }
    }

    // Check if inside a song block row: last non-whitespace before cursor ends
    // with `|` or we're after `| ` at start of a song row
    if (trimmed.ends_with('|')
        || (trimmed.rfind('|').is_some() && is_inside_song_block(source, byte_offset)))
        && is_inside_song_block(source, byte_offset)
    {
        return Some(BackwardContext::SongRow);
    }

    None
}

/// Heuristic: check if the cursor is inside a song block by scanning backward
/// for `song <name> {` without a closing `}`.
fn is_inside_song_block(source: &str, byte_offset: usize) -> bool {
    let before = &source[..byte_offset];
    // Find the last `song ` keyword
    if let Some(song_pos) = before.rfind("song ") {
        let after_song = &before[song_pos..];
        let open_braces = after_song.matches('{').count();
        let close_braces = after_song.matches('}').count();
        open_braces > close_braces
    } else {
        false
    }
}

fn dedup_completion_items(items: &mut Vec<CompletionItem>) {
    let mut seen = std::collections::HashSet::new();
    items.retain(|item| seen.insert(item.label.clone()));
    items.sort_by(|a, b| a.label.cmp(&b.label));
}

// ─── Formatting helpers (shared with hover) ──────────────────────────────

pub(crate) fn format_parameter_kind(kind: &patches_core::ParameterKind) -> String {
    match kind {
        patches_core::ParameterKind::Float { min, max, default } => {
            format!("float ({min}..{max}, default {default})")
        }
        patches_core::ParameterKind::Int { min, max, default } => {
            format!("int ({min}..{max}, default {default})")
        }
        patches_core::ParameterKind::Bool { default } => {
            format!("bool (default {default})")
        }
        patches_core::ParameterKind::Enum { variants, default } => {
            let vs = variants.join(" | ");
            format!("enum ({vs}, default {default})")
        }
        patches_core::ParameterKind::String { default } => {
            format!("string (default \"{default}\")")
        }
        patches_core::ParameterKind::Array { length, .. } => {
            format!("array (length {length})")
        }
        patches_core::ParameterKind::File { extensions } => {
            let exts = extensions.join(", ");
            format!("file ({exts})")
        }
        patches_core::ParameterKind::SongName => "song name".to_string(),
    }
}

pub(crate) fn cable_kind_str(kind: &patches_core::CableKind) -> &'static str {
    match kind {
        patches_core::CableKind::Mono => "mono",
        patches_core::CableKind::Poly => "poly",
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis;
    use crate::ast_builder;
    use crate::parser::language;
    use patches_modules::default_registry;
    use tree_sitter::Parser;

    fn setup(source: &str) -> (Tree, SemanticModel, Registry) {
        let mut parser = Parser::new();
        parser.set_language(&language()).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let (file, _) = ast_builder::build_ast(&tree, source);
        let registry = default_registry();
        let model = analysis::analyse(&file, &registry);
        (tree, model, registry)
    }

    #[test]
    fn completions_for_module_type() {
        let source = "patch {\n    module osc : \n}";
        let (tree, model, registry) = setup(source);
        let byte_offset = source.find(": \n").unwrap() + 2;
        let items = compute_completions(&tree, source, byte_offset, &model, &registry);
        assert!(
            items.iter().any(|i| i.label == "Osc"),
            "expected Osc in completions, got: {:?}",
            items.iter().map(|i| &i.label).collect::<Vec<_>>()
        );
    }

    #[test]
    fn completions_while_typing_module_type() {
        let source = "patch {\n    module x : Pitch\n}";
        let (tree, model, registry) = setup(source);
        let byte_offset = source.find("Pitch").unwrap() + 5;
        let items = compute_completions(&tree, source, byte_offset, &model, &registry);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(
            labels.contains(&"PitchShift"),
            "expected PitchShift in completions, got: {labels:?}"
        );
        assert!(
            labels.contains(&"ConvReverb"),
            "expected ConvReverb in completions, got: {labels:?}"
        );
    }

    #[test]
    fn completions_for_at_block_aliases() {
        let source =
            "patch {\n    module mx : Mixer(channels: [drum, bass]) {\n        @\n    }\n}";
        let (tree, model, registry) = setup(source);
        let byte_offset = source.find('@').unwrap() + 1;
        let items = compute_completions(&tree, source, byte_offset, &model, &registry);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(
            labels.contains(&"drum"),
            "expected drum in completions, got: {labels:?}"
        );
        assert!(
            labels.contains(&"bass"),
            "expected bass in completions, got: {labels:?}"
        );
    }

    #[test]
    fn completions_for_parameters() {
        let source = "patch {\n    module osc : Osc { }\n}";
        let (tree, model, registry) = setup(source);
        let byte_offset = source.find("{ }").unwrap() + 2;
        let items = compute_completions(&tree, source, byte_offset, &model, &registry);
        assert!(
            items.iter().any(|i| i.label == "frequency"),
            "expected frequency in completions, got: {:?}",
            items.iter().map(|i| &i.label).collect::<Vec<_>>()
        );
    }

    #[test]
    fn completions_for_port_after_dot() {
        let source = "patch {\n    module osc : Osc\n    module out : AudioOut\n    osc.\n}";
        let (tree, model, registry) = setup(source);
        let byte_offset = source.find("osc.\n").unwrap() + 4;
        let items = compute_completions(&tree, source, byte_offset, &model, &registry);
        assert!(
            items.iter().any(|i| i.label == "sine"),
            "expected sine in completions, got: {:?}",
            items.iter().map(|i| &i.label).collect::<Vec<_>>()
        );
    }

    #[test]
    fn completions_for_port_index_aliases() {
        let source =
            "patch {\n    module mix : Mixer(channels: [drum, bass])\n    mix.out[\n}";
        let (tree, model, registry) = setup(source);
        let byte_offset = source.find("out[").unwrap() + 4;
        let items = compute_completions(&tree, source, byte_offset, &model, &registry);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(
            labels.contains(&"drum"),
            "expected drum in completions, got: {labels:?}"
        );
        assert!(
            labels.contains(&"bass"),
            "expected bass in completions, got: {labels:?}"
        );
    }

    #[test]
    fn completions_for_dollar_dot_in_template() {
        let source = "template voice {\n    in: voct, gate\n    out: audio\n    module osc : Osc\n    $.\n}\npatch {}";
        let (tree, model, registry) = setup(source);
        let byte_offset = source.find("$.").unwrap() + 2;
        let items = compute_completions(&tree, source, byte_offset, &model, &registry);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(
            labels.contains(&"voct"),
            "expected voct in completions, got: {labels:?}"
        );
        assert!(
            labels.contains(&"gate"),
            "expected gate in completions, got: {labels:?}"
        );
        assert!(
            labels.contains(&"audio"),
            "expected audio in completions, got: {labels:?}"
        );
    }

    #[test]
    fn completions_for_pattern_names_in_song_row() {
        let source = "pattern drums {\n    kick: x . x .\n}\n\nsong my_song {\n    | ch |\n    | \n}\n\npatch {}";
        let (tree, model, registry) = setup(source);
        // Position cursor inside the song row after `| `
        let byte_offset = source.find("| \n}").unwrap() + 2;
        let items = compute_completions(&tree, source, byte_offset, &model, &registry);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(
            labels.contains(&"drums"),
            "expected drums in completions, got: {labels:?}"
        );
    }

    #[test]
    fn completions_for_song_names_in_master_sequencer() {
        let source = "song my_song {\n    | ch |\n}\n\npatch {\n    module seq : MasterSequencer(channels: [ch]) {\n        song: \n    }\n}";
        let (tree, model, registry) = setup(source);
        let byte_offset = source.find("song: \n").unwrap() + 6;
        let items = compute_completions(&tree, source, byte_offset, &model, &registry);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(
            labels.contains(&"my_song"),
            "expected my_song in completions, got: {labels:?}"
        );
    }
}
