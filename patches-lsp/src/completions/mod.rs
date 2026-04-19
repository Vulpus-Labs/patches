//! Completion engine for the patches DSL.
//!
//! Provides context-sensitive completions for module types, parameters,
//! ports, shape arguments, and template ports.
//!
//! # Two dispatch paths
//!
//! Completions flow through two distinct paths:
//!
//! 1. **Parsed-input dispatch** — `classify_cursor` returns a
//!    [`crate::tree_nav::CursorContext`] built from the tree-sitter parse
//!    tree. [`compute_completions`] matches on the variant and hands off to
//!    the appropriate completer. This covers every case where tree-sitter
//!    produced a usable node (including error-recovered nodes that still
//!    carry structure, like an empty type slot).
//!
//! 2. **Incomplete-input fallback** — [`scan_backward_for_context`] inspects
//!    the source text behind the cursor when tree-sitter has no node (e.g.
//!    `osc.` with the cursor just past the dot, or `$.` inside a template
//!    before a port identifier exists). Folding this into the classifier
//!    would require either ERROR-node text inspection or richer tree-sitter
//!    error recovery; both are out of scope for E084.

mod backward_scan;
mod module_types;
mod params;
mod ports;
mod shape;

use patches_registry::Registry;
use tower_lsp::lsp_types::*;
use tree_sitter::Tree;

use crate::analysis::SemanticModel;
use crate::tree_nav::{
    classify_cursor, is_master_sequencer, module_instance_name, CursorContext,
};

use backward_scan::{scan_backward_for_context, BackwardContext};
use module_types::complete_module_types;
use params::{
    complete_parameters, complete_pattern_names, complete_song_names, is_after_param_colon,
};
use ports::{complete_port_ref, complete_ports, complete_template_ports};
use shape::{
    complete_at_block_aliases, complete_port_index_aliases, complete_shape_args, is_after_at_sign,
};

/// Determine the completion context from the cursor position and return items.
pub(crate) fn compute_completions(
    tree: &Tree,
    source: &str,
    byte_offset: usize,
    model: &SemanticModel,
    registry: &Registry,
) -> Vec<CompletionItem> {
    match classify_cursor(tree, byte_offset) {
        CursorContext::ModuleType { .. } | CursorContext::ModuleTypeSlot { .. } => {
            return complete_module_types(model, registry);
        }
        CursorContext::ParamBlock { module_decl, .. } => {
            return complete_param_block(module_decl, source, byte_offset, model);
        }
        CursorContext::ShapeBlock { .. } => {
            return complete_shape_args();
        }
        CursorContext::PortRef { port_ref_node, .. } => {
            return complete_port_ref(source, byte_offset, port_ref_node, tree, model);
        }
        CursorContext::SongRow { .. } => {
            return complete_pattern_names(model);
        }
        CursorContext::ModuleName { .. } | CursorContext::Unknown => {}
    }

    // Incomplete-input fallback: tree-sitter produced no classifiable node
    // for this offset. Use the textual backward scanner.
    if let Some(ctx) = scan_backward_for_context(source, byte_offset) {
        return match ctx {
            BackwardContext::ModuleColon | BackwardContext::ModuleTypeName => {
                complete_module_types(model, registry)
            }
            BackwardContext::Dot(module_name) => {
                complete_ports(&module_name, model, byte_offset, source, tree)
            }
            BackwardContext::DollarDot => complete_template_ports(source, byte_offset, tree, model),
            BackwardContext::PortIndex(module_name) => {
                complete_port_index_aliases(&module_name, model)
            }
            BackwardContext::SongRow => complete_pattern_names(model),
        };
    }

    vec![]
}

/// Dispatch `ParamBlock`-kind completions: shape-alias list (`@`),
/// MasterSequencer `song:` slot, or general parameter names.
fn complete_param_block(
    module_decl: tree_sitter::Node,
    source: &str,
    byte_offset: usize,
    model: &SemanticModel,
) -> Vec<CompletionItem> {
    if is_after_at_sign(source, byte_offset) {
        return complete_at_block_aliases(module_decl, source);
    }
    if is_master_sequencer(module_decl, source)
        && is_after_param_colon(source, byte_offset, "song")
    {
        return complete_song_names(model);
    }
    complete_parameters(module_instance_name(module_decl, source), model)
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
        let source = "pattern drums {\n    kick: x . x .\n}\n\nsong my_song(ch) {\n    play {\n        \n    }\n}\n\npatch {}";
        let (tree, model, registry) = setup(source);
        // Position cursor inside the play block before the closing brace.
        let byte_offset = source.find("        \n    }").unwrap() + 8;
        let items = compute_completions(&tree, source, byte_offset, &model, &registry);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(
            labels.contains(&"drums"),
            "expected drums in completions, got: {labels:?}"
        );
    }

    #[test]
    fn completions_for_song_names_in_master_sequencer() {
        let source = "song my_song(ch) {\n    play {}\n}\n\npatch {\n    module seq : MasterSequencer(channels: [ch]) {\n        song: \n    }\n}";
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
